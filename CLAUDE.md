## Project Context
- Pure Rust implementation of the F1R3FLY.io blockchain platform
- Extracted from the `rust/dev` branch of [f1r3fly](https://github.com/F1R3FLY-io/f1r3fly) as a standalone Rust workspace
- **No Nix, no SBT, no Scala** — this repo builds with standard Rust tooling (cargo + system deps)
- Implements concurrent smart contract execution with Byzantine Fault Tolerant consensus
- If the user does not provide enough information with their prompts, ask the user to clarify before executing the task

## Architecture Overview

# F1R3node Rust — Pure Rust Blockchain Node

## Platform Requirements
- **Rust nightly** — pinned in `rust-toolchain.toml` (currently nightly-2026-02-09)
- **protoc** — Protocol Buffers compiler (required by build.rs in node, models, comm)
- **OpenSSL** — headers and libraries (required by crypto crate)
- **pkg-config** — helps cargo find native libraries
- **just** — command runner for local development (optional but recommended)
- **Docker** — for running node networks (optional)

### macOS Quick Setup
```bash
brew install protobuf openssl pkg-config just
```

### Workspace Crates (11 crates)
| Crate | Purpose |
|-------|---------|
| `node` | Main binary: CLI, gRPC server, HTTP API, REPL, metrics |
| `casper` | CBC Casper consensus, block validation, finalization |
| `rholang` | Rholang smart contract language interpreter |
| `rspace++` (`rspace_plus_plus`) | High-performance tuple space storage (LMDB/heed backend) |
| `models` | Protobuf data models, gRPC service definitions |
| `crypto` | Ed25519, Secp256k1, Blake2b, TLS certificate generation |
| `comm` | P2P networking, Kademlia discovery, custom TLS validation |
| `block-storage` | Block persistence and retrieval |
| `shared` | Common utilities, middleware, metrics helpers |
| `graphz` | DAG traversal and graph algorithms |
| `rspace_plus_plus_rhotypes` | Type bridge between RSpace and Rholang |

### Multi-Consensus Design
Four consensus mechanisms, all implemented in Rholang:
1. **Cordial Miners** — Cooperative, energy-efficient
2. **Casper CBC** — BFT with mathematical safety proofs (primary, implemented)
3. **RGB PSSM** — Client-side validation with Bitcoin anchoring
4. **Casanova** — Adaptive consensus for high-performance scenarios

### External Dependency: rholang-parser
The Rholang parser is an external crate:
```toml
rholang-parser = { git = "https://github.com/F1R3FLY-io/rholang-rs", rev = "d25f953a" }
```
Used by `rholang` and `rspace++` crates.

## Development Commands

```bash
# Build
cargo build                          # Debug build
cargo build --release                # Release build
cargo build -p node                  # Just the node binary
just build                           # Release build via Justfile

# Test
cargo test                           # All tests
cargo test -p casper                 # Specific crate
cargo test --release                 # Release mode (faster rholang tests)
./scripts/run_rust_tests.sh          # Full test suite script

# Run
just run-standalone                  # Build + run standalone node
just run-standalone-debug            # Debug mode
just clean-standalone                # Reset node data

# Docker
docker build -f node/Dockerfile -t f1r3fly-rust-node:local .
docker compose -f docker/standalone.yml up
docker compose -f docker/shard.yml up
```

## Code Style and Standards

### Rust Guidelines
- **No comments** unless explicitly requested by user
- Zero-cost abstractions, proper ownership
- Async/await with Tokio runtime
- Error handling: `eyre` for application errors, `thiserror` for library errors
- Logging: `tracing` crate throughout
- Serialization: `prost` for protobuf, `serde` for JSON/bincode

### Build Scripts
Three crates have `build.rs` for protobuf code generation:
- `node/build.rs` — `repl.proto`, `lsp.proto`
- `models/build.rs` — `RhoTypes.proto`, `CasperMessage.proto`, `DeployServiceV1.proto`, etc.
- `comm/build.rs` — `kademlia.proto`

### Important Configuration
- `.cargo/config.toml` — stack size (8MB for rholang recursion), native CPU features
- `rust-toolchain.toml` — nightly channel pin
- `Cross.toml` — cross-compilation for amd64/arm64

## Network Ports
| Port | Service |
|------|---------|
| 40400 | Protocol Server |
| 40401 | gRPC External API |
| 40402 | gRPC Internal API |
| 40403 | HTTP REST API |
| 40404 | Peer Discovery |

## Security
- Never log or expose private keys
- Validate all user inputs and state transitions
- TLS 1.3 for P2P communications
- Capability-based security in Rholang contracts

## Git and Version Control

### Commit Messages
- Use `[agent]` prefix in agentic mode
- Do NOT include Claude Code attribution footer or emoji
- Do NOT include Co-Authored-By lines
- Keep commit messages clean and professional

### Branch Strategy
- `main` — stable releases
- `master` — current working branch
- Feature branches for development

## Relationship to f1r3node
This repo was extracted from `F1R3FLY-io/f1r3fly` (`rust/dev` branch). Key differences:
- **Removed**: Nix flake, SBT build, Scala source, `.envrc`, JVM tooling
- **Kept**: All 11 Rust crates, Cargo workspace, protobuf definitions, Docker configs, docs
- **Added**: Native dependency install instructions (Homebrew, apt)

### Key Principles

1. **Stigmergic Collaboration**: Coordinate with other agents through shared `.md` files
2. **Document-First**: Create design docs and specifications BEFORE implementation
3. **Signal vs. Slop**: Maximize code that solves problems; avoid over-engineering
4. **Acceptance Criteria**: Define measurable success criteria in task definitions

### Standard Document Structure

| Document | Purpose | Location |
|----------|---------|----------|
| User Stories | Business needs and acceptance criteria | `docs/UserStories.md` |
| Tasks/Epochs | Implementation tracking | `docs/ToDos.md` |
| Completed Work | Historical reference | `docs/CompletedTasks.md` |
| Backlog | Deferred items | `docs/Backlog.md` |
| Work Logs | Session progress | `docs/work-logs/*.md` |
| Discoveries | Shared findings | `docs/discoveries/*.md` |

### Before Starting Work

1. **Read `docs/ToDos.md`** to check task status and claims
2. **Check `docs/work-logs/`** for existing progress on related tasks
3. **Review `docs/discoveries/`** for relevant context from other agents

### When Claiming a Task

Update the task in `docs/ToDos.md`:

```yaml
---
id: TASK-001
status: in_progress          # Changed from 'pending'
claimed_by: claude-session-a1b2c3  # See Implementer Identification format
claimed_at: 2025-01-15T10:00:00Z
# Other valid claimed_by formats:
#   human-jeff@example.com        # Human (git config --get user.email)
#   design-sprint/researcher      # Agent team member ({team}/{name})
---
```

### During Work

1. **Create work log** at `docs/work-logs/task-{id}-{timestamp}.md`
2. **Document discoveries** in `docs/discoveries/` for other agents
3. **Update blockers** if you encounter dependencies

### Before Pausing/Completing

Update your work log with handoff notes:

```yaml
---
handoff_status: ready | paused | blocked
next_steps:
  - What remains to be done
---
```

### Configuration File Conventions

When creating or modifying configuration files, follow these conventions to respect existing project preferences:

**JSON Format Preference Order:**

1. **Check for existing files first**: Before creating any `.json` file, check if `.jsonc` or `.json5` variants exist
2. **Prefer existing format**: If `config.jsonc` or `config.json5` exists, use that format instead of creating `config.json`
3. **Default to JSONC**: When creating new config files, prefer `.jsonc` (JSON with Comments) for better maintainability

**Why This Matters:**
- Projects may have established preferences for comment-supporting JSON formats
- Creating duplicate configs (e.g., both `biome.json` and `biome.jsonc`) causes confusion
- JSONC allows inline documentation which improves maintainability

**Examples:**

| If exists... | Don't create... | Instead... |
|--------------|-----------------|------------|
| `biome.jsonc` | `biome.json` | Edit the existing `biome.jsonc` |
| `tsconfig.json5` | `tsconfig.json` | Edit the existing `tsconfig.json5` |
| `eslint.config.jsonc` | `eslint.config.json` | Edit the existing file |
| Nothing | - | Create new file as `.jsonc` when comments are useful |

**File Discovery Pattern:**

Before creating any config file, check for variants:
```bash
# Check for config variants (example for biome)
ls biome.json biome.jsonc biome.json5 2>/dev/null
```

This applies to all slash commands and scripts that create configuration files.

#### Git Operations
- `/quick-commit` - Stage and commit changes (required in safe mode)
- `/recursive-push` - Push across repositories

#### Task Management
- `/nextTask` - Find and select next task to work on
- `/implement` - Begin implementation of a task
- `/epoch-review` - Preview and summarize epochs
- `/epoch-hygiene` - Archive completed epochs

#### Workspace Sync
- `/harmonize` - Sync workspace policies into this repo
- `/multi-repo-sync` - Workspace-wide sync orchestration

[OPTIONAL_COMMANDS]

### PII Guidelines for Contributors

**CRITICAL - Before submitting any contribution:**

Contributors MUST ensure their code, commits, and documentation do NOT contain PII:

**Check before committing:**
- [ ] No absolute file paths with usernames in code or documentation
- [ ] No personal email addresses in code (use generic examples like `user@example.com`)
- [ ] No real user data in tests or examples (use synthetic/fake data only)
- [ ] No PII in log statements (sanitize or use user IDs instead)
- [ ] No PII in error messages or stack traces
- [ ] No PII in code comments or documentation
- [ ] No credentials, tokens, or secrets in code (use environment variables)
- [ ] No IP addresses, MAC addresses, or device identifiers in examples

**If you accidentally committed PII:**
1. **DO NOT** push to remote repository
2. Use `git reset` to remove the commit
3. If already pushed, contact maintainers immediately
4. Repository history may need to be rewritten to remove PII

**Use these instead:**
- File paths: Use relative paths or generic placeholders (`[WORKSPACE_ROOT]/project/`)
- Email addresses: Use `user@example.com`, `admin@example.com`
- Names: Use `John Doe`, `Jane Smith`, `User123`
- Phone numbers: Use `+1-555-0100` (officially reserved for examples)
- IP addresses: Use reserved ranges (`192.0.2.1`, `198.51.100.1`, `203.0.113.1`)
- Dates: Use recent but generic dates, not specific personal dates

**For test data:**
- Use test data generators that create realistic but fake data
- Use well-known test fixtures (e.g., `test@example.com`)
- Never use production or real user data in development/testing

# important-instruction-reminders
Do what has been asked; nothing more, nothing less.
NEVER create files unless they're absolutely necessary for achieving your goal.
ALWAYS prefer editing an existing file to creating a new one.
NEVER proactively create documentation files (*.md) or README files. Only create documentation files if explicitly requested by the User.
Before making any code changes, first state: (1) which files you plan to modify, (2) what approach you'll take, (3) any assumptions you're making. Wait for my confirmation before proceeding. For simple single-file edits, a one-line summary is sufficient.
