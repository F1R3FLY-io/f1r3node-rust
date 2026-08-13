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

## Git Interaction

- Do not run `git add`, `git commit`, or `git push` unless explicitly requested
- Commit consent applies to one commit. Do not create a commit without a clear request for that commit.
- This rule includes merge commits and plumbing equivalents such as `commit-tree` and `update-ref`.
- Consent from a plan, an earlier commit, or a prior merge does not apply.
- A request to resolve merge conflicts authorizes conflict resolution only.
- Resolve the files, verify the build, report the result, and stop before the merge commit.
- Do not use `git stash pop|drop|clear|branch`. These commands can lose changes.
- Other stash writes require user confirmation. You can use `stash list` and `stash show`.
- See `CLAUDE.md` § "Git Interaction Policy" for the full rules

## Subagent Usage

- Only use a subagent if the user has explicitly told you to do so
- Do not delegate work to subagents on your own initiative. Perform tasks directly by default

## Security
- Never log or expose private keys
- Validate all user inputs and state transitions
- TLS 1.3 for P2P communications
- Capability-based security in Rholang contracts
