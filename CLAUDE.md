# F1R3node Rust — Pure Rust Blockchain Node

## Project Context
- Pure Rust implementation of the F1R3FLY.io blockchain platform
- Extracted from the `rust/dev` branch of [f1r3fly](https://github.com/F1R3FLY-io/f1r3fly) as a standalone Rust workspace
- **No Nix, no SBT, no Scala** — this repo builds with standard Rust tooling (cargo + system deps)
- Implements concurrent smart contract execution with Byzantine Fault Tolerant consensus
- If the user does not provide enough information with their prompts, ask the user to clarify before executing the task

**Glossary:** Project terminology lives in [docs/Glossary.md](docs/Glossary.md).
This glossary is load-bearing: documentation, ADRs, and code reviews cite
its anchors. See `**Preferred usage.**` subsections for canonical vs. avoided
phrasings. Mathematical notation and theorem naming remain in
`docs/theory/slashing/design/02-glossary-and-notation.md` pending unification
(BACKLOG-DOC-001).

## Architecture Overview

The workspace separates consensus, execution, storage, networking, and node services into focused Rust crates.

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

### Workspace Crates (10 crates)
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
docker build -f node/Dockerfile -t f1r3fly-rust:local .
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

### Recommended Claude Code local settings
Add to `.claude/settings.local.json` (personal, not committed) when working
in this repo with Claude Code:

```json
{
  "fileCheckpointingEnabled": false,
  "env": {
    "BASH_DEFAULT_TIMEOUT_MS": "1200000",
    "BASH_MAX_TIMEOUT_MS": "1800000"
  }
}
```

- `fileCheckpointingEnabled: false` — Claude Code's checkpointing/rewind
  feature runs `git stash` + `git reset --hard` against the workspace repo
  around tool events, taking real `.git/index.lock` locks that collide with
  concurrent git commands ("Unable to create index.lock"; see
  anthropics/claude-code#68315). Disabling it trades away `/rewind` file
  restore in this repo.
- Bash timeouts raised to 20/30 min — the pre-push hook runs the full
  release test suite for all 11 crates (~9 min, longer on cold caches),
  which exceeds the default 10-minute window and gets a `git push` killed
  mid-gate when run through Claude Code.

Both settings take effect at the next session start.

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

### Git Interaction Policy (agents)
- Use `/quick-commit` for git add/commit operations
- Use `/recursive-push` for git push operations
- Do not run `git add`, `git commit`, or `git push` directly unless explicitly requested
- **Commit consent is per-commit**: never create a commit — including merge
  commits and plumbing equivalents (`git commit-tree`, `git update-ref`) —
  without the user invoking `/quick-commit` or giving an unambiguous
  per-commit "yes". Consent does not carry over from a plan, an earlier
  commit, or a previous merge in the same session.
- **Merge conflicts**: a request to "resolve the merge conflicts" authorizes
  conflict resolution only — resolve the files, verify the build, report,
  then STOP before the merge commit. The user running `git merge` in their
  own terminal is not a request for the agent to act.
- `git mv` is permitted but requires user confirmation
- `git stash`:
  - `git stash list`, `git stash show` are permitted (read-only)
  - `git stash`, `git stash push|save|apply` require user confirmation
  - `git stash pop|drop|clear|branch` are blocked (destructive; can silently lose uncommitted work)
- `git worktree`: NEVER create a worktree (`git worktree add`) unless the
  user explicitly asks for one. All work happens in this single checkout —
  create new branches here, not in sibling directories. Worktrees fragment
  local state (branches pinned to hidden checkouts, invisible to
  `/recursive-push` discovery, and a past root cause of an accidental push
  to a protected branch). `git worktree list` is permitted (read-only);
  `git worktree remove|prune` requires user confirmation.
- **Exception:** In agentic mode (`claude-agentic`), all restrictions are lifted
- The workspace stigmergic guidance to "commit frequently" applies to humans
  and fully-autonomous (YOLO/worktree) modes; in interactive sessions it is
  overridden by the consent rules above.

**Full Documentation**: [Git Interaction Policy](https://gitlab.com/smart-assets.io/gitlab-profile/-/blob/master/docs/common/git-interaction-policy.md) (canonical; also available at `../../SA/top-level-gitlab-profile/docs/common/git-interaction-policy.md` in a multi-repo workspace checkout).

### Commit Messages
- Use `[agent]` prefix in agentic mode
- Do NOT include Claude Code attribution footer or emoji
- Do NOT include Co-Authored-By lines
- Keep commit messages clean and professional

### Branch Strategy
- `master` — default branch and release line; maintainers promote `dev` → `master`
- `dev` — integration branch; feature and fix PRs target this
- Feature branches (`feature/`, `fix/`, `docs/`, `perf/`, `chore/`) branch from and target `dev`
- `hotfix/` branches from and target `master`, then `master` is merged back into `dev`
- There is no `main` branch, and `staging` is deprecated (fully contained in `dev`)

## Relationship to f1r3node
This repo was extracted from `F1R3FLY-io/f1r3fly` (`rust/dev` branch). Key differences:
- **Removed**: Nix flake, SBT build, Scala source, `.envrc`, JVM tooling
- **Kept**: All 10 Rust crates, Cargo workspace, protobuf definitions, Docker configs, docs
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
| Tasks/Epics | Implementation tracking | `docs/ToDos.md` |
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
- `/epic-review` - Preview and summarize epics
- `/epic-hygiene` - Archive completed epics

#### Workspace Sync
- `/harmonize` - Sync workspace policies into this repo
- `/multi-repo-sync` - Workspace-wide sync orchestration

[OPTIONAL_COMMANDS]

## PII Guidelines for Contributors

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

## AI Artifact Generation Guidelines

**Core strategy:** Default to **Markdown + Mermaid** as the source of truth for all generated artifacts. Use **HTML** only when high engagement or advanced interactivity is required.

**Preferred formats:**

| Format | Use for | Notes |
|--------|---------|-------|
| **Markdown + Mermaid** | Primary. Diagrams (flowcharts, sequences, architecture, timelines, Gantt, ERDs), structured documents, plans, specs | Relative links (`./images/`, `./docs/`) and GitHub/GitLab raw URLs for local/cloud asset referencing |
| **HTML (CSS/JS + embedded Mermaid)** | Secondary. Interactive dashboards, prototypes, dynamic reviews, stakeholder deliverables | When visual polish and engagement are critical (tabs, sliders, clickable elements) |

**Hybrid rule:** Always produce Markdown as the canonical, Git-friendly version first. Generate a self-contained HTML export on request.

**Key principles:**

- Prioritize human readability, editability, and Git compatibility (clean diffs, relative paths, native rendering on GitHub/GitLab).
- Maximize information density while avoiding text walls — convert complex information into Mermaid diagrams.
- Support seamless referencing of local files and cloud artifacts (images, other docs, raw Git content).
- Favor Markdown for internal/agent use and long-term storage (token efficiency).
- Use HTML when delivering to stakeholders or for living documents (engagement).

**Output guidance:** When creating artifacts, ask whether HTML interactivity is needed. Default to clean Markdown with embedded Mermaid unless specified.

<!-- ste-policy: required -->

## Simplified Technical English
<!-- ste-policy: full -->

Smart Assets uses ASD-STE100 Simplified Technical English, Issue 9, dated January 2025, for applicable English technical prose.

This policy is a Smart Assets applicability profile. The ASD standard remains the authoritative source for its rules and controlled dictionary.

Get the current standard from the [official ASD-STE100 website](https://www.asd-ste100.org/) or its [official downloads page](https://www.asd-ste100.org/STE_downloads.html).

ASD owns the standard and the ASD-STE100 trademark. Do not copy its controlled dictionary, examples, or substantial rule text into this repository.

### Applicability

Use this policy for English technical prose that an assistant creates or substantially rewrites, including:

- Assistant responses
- Technical documentation
- Plans and specifications
- Procedures and instructions
- Warnings and cautions
- Review findings
- User-facing explanations and status messages.

Preserve unaffected legacy prose. Apply this policy to the text that the assistant adds or substantially changes.

If the user explicitly requests another language, use that language. If you report STE status, mark STE as not applicable to that output.

This policy does not apply to:

- Code, identifiers, commands, flags, paths, URLs, and schema keys
- Data formats, exact test fixtures, and machine-generated text
- Verbatim quotations and user-supplied text
- Third-party names, standard titles, and legal or license text.

Do not change technical meaning, safety controls, legal meaning, or exact user requirements only to satisfy a language rule.

### Words and terminology

Use the official Issue 9 dictionary as the source for general approved words. Do not copy the dictionary into project files.

Use a word only with its approved part of speech, meaning, and form. Use American English spelling unless another directive controls the text.

Use approved technical nouns and technical verbs for the applicable subject field. Record recurring Smart Assets terms in `docs/Glossary.md`.

Use one technical noun for one concept. Do not replace a canonical term with a synonym only for stylistic variation.

Keep a new technical noun short and easy to understand. Use no more than three words unless an approved term requires more words.

Do not use regional words, slang, or unexplained jargon. Define an unavoidable abbreviation at its first use.

Use technical nouns as nouns. Use technical verbs as verbs and only in their approved software-development meaning.

### Grammar and sentences

Use active voice. In descriptive text, use passive voice only when the agent is unknown or technically unimportant.

Use simple verb forms and tenses. Avoid progressive and other complex constructions unless an exempt technical term requires them.

Use a direct verb to describe an action. Do not hide an action in an abstract noun phrase.

Write complete sentences. Do not omit articles, subjects, verbs, or necessary nouns to make a sentence shorter.

Do not use contractions. Do not use semicolons.

Keep each sentence focused on one subject. Use a vertical list when one sentence would contain complex items or actions.

Make each pronoun refer to one clear noun. Repeat the technical noun when a pronoun can have more than one meaning.

### Procedures

Use a maximum of 20 words in each procedural sentence. Use one instruction in each sentence unless actions occur at the same time.

Start each instruction with an imperative verb. Put a necessary condition before the instruction and separate it with a comma.

Use numbered steps when sequence is important. Keep information-only notes separate from instructions, requirements, limits, and safety information.

### Descriptions

Use a maximum of 25 words in each descriptive sentence. Give information gradually and keep one topic in each paragraph.

Start each paragraph with its topic. Use no more than six sentences in one paragraph.

Use consistent key words to connect related sentences. Start a new paragraph when the topic changes.

### Safety information

Use the project-approved risk word or symbol. Start with a clear command or condition, and then state the risk or possible result.

Do not hide safety information in a note. Keep safety controls and their consequences explicit.

### Verification

An **STE Check** is deterministic. It can check policy coverage, sentence limits, paragraph limits, contractions, semicolons, and selected exemptions.

An **STE Review** is semantic. A human reviewer must check vocabulary, meaning, active voice, referents, terminology, and technical accuracy.

For committed prompt and policy files, run the repository STE Check. Review all applicable new or substantially rewritten prose before completion.

A checker cannot establish full ASD-STE100 conformance. Do not claim that text is ASD-STE100 compliant only because an automated check passes.

## UI Test Assertions

React UI tests must prove user-observable DOM structure and state. Do not test incidental copy or formatted sample values.
Use exact text or value assertions only when the test covers copy, formatting, or content transformation.

- **Preferred:** Use accessibility-first queries for DOM elements and states. Examples include roles, labels, focus state, and ARIA relationships.
- **Preferred:** Assert semantic regions/components are present and wired correctly (tabs, tabpanels, dialogs, forms, buttons, lists), then assert behavior through state changes or callback/data-source calls.
- **Acceptable:** Use `data-testid` when no accessible handle exists. You can also use it for presentational elements or stable component boundaries.
- **Acceptable:** HTML `id` attributes for form elements and ARIA relationships.
- **Avoid:** `getByText`/`queryByText` for literal strings that are merely display copy, repeated metrics, formatted numbers, or mock-data values.
- **Avoid:** Do not use `getAllByText(...).length` as a substitute for a meaningful DOM assertion. It couples tests to duplicated visual text instead of behavior.
- **Avoid:** CSS class selectors that may change with styling updates.

Examples:

```tsx
// GOOD: accessibility-first DOM element + state
expect(screen.getByRole("button", { name: "Refresh agents" })).toBeEnabled();

// ACCEPTABLE: fallback when no accessible role/name fits
expect(screen.getByTestId("cost-optimization-panel")).toBeInTheDocument();

// GOOD for data-flow behavior: verify source + rendered component boundary,
// not a duplicated metric string like "7.5%".
expect(fetchQualityPipeline).toHaveBeenCalledTimes(1);
expect(screen.getByRole("tablist", { name: "Quality Pipeline tabs" })).toBeInTheDocument();
expect(screen.getByRole("tab", { name: /overview/i })).toHaveAttribute("aria-selected", "true");

// BAD: brittle copy assertion
expect(screen.getByText("Priority set by BountyForge routing")).toBeInTheDocument();

// BAD: brittle mock-value / duplicated visual text assertion
expect(screen.getByText("7.5%")).toBeInTheDocument();
expect(screen.getAllByText("7.5%").length).toBeGreaterThan(0);
```

**When exact text is appropriate:** Use exact text assertions only when the acceptance criteria is about user-facing copy, accessibility name computation, validation messages, formatting rules, or transformed content. Prefer scoping with `within(...)` to a semantic container so the assertion remains tied to the behavior under test.

## Worktree Policy

Git worktrees are an **agentic-mode-only** tool in this workspace. In safe /
interactive mode:

- **NEVER create a git worktree** unless the user explicitly asks for one in
  their own words. This covers every creation path equally: `git worktree add`,
  harness-native tools (e.g. an `EnterWorktree` tool, subagent
  `isolation: "worktree"`, workflow worktree isolation), and any script that
  wraps them.
- All work happens in the single main checkout on the current branch.
- `git worktree list` is read-only and permitted.
- `git worktree remove` and `git worktree prune` require explicit user
  confirmation.

**Why:** worktrees fragment local state, are invisible to `/recursive-push`
repository discovery, and were the root cause of an accidental push to a
protected branch. A worktree created silently by an assistant is a worktree
nobody pushes, cleans up, or audits.

**Exception:** In agentic mode (`claude-agentic`), all restrictions are lifted.
YOLO mode runs *inside* a worktree that the human created. Running in a worktree
never authorizes the creation of more worktrees.

Canonical policy: [Git Interaction Policy](https://gitlab.com/smart-assets.io/gitlab-profile/-/blob/master/docs/common/git-interaction-policy.md)
(Worktrees section).

## grepai - Semantic Code Search

`grepai` is an optional, MIT-licensed semantic-search tool. If it is installed
and indexed locally for this repo, prefer it for intent-based code exploration.
If it is unavailable, use your harness's native search and file-reading tools.
The tool is recommended but never required. Nothing here is harness-specific -
substitute your tool's equivalents for the generic actions described below.
Setup (with the privacy-first local Ollama embedder as default) lives in the
CLI Setup guide's "Optional: Semantic Code Search (grepai)" section.

# important-instruction-reminders
Do what has been asked; nothing more, nothing less.
NEVER create files unless they're absolutely necessary for achieving your goal.
ALWAYS prefer editing an existing file to creating a new one.
NEVER proactively create documentation files (*.md) or README files. Only create documentation files if explicitly requested by the User.
Before making any code changes, first state: (1) which files you plan to modify, (2) what approach you'll take, (3) any assumptions you're making. Wait for my confirmation before proceeding. For simple single-file edits, a one-line summary is sufficient.
