---
cross_references:
  - purpose: current
    repository: https://gitlab.com/smart-assets.io/gitlab-profile
    checkout: ../../SA/top-level-gitlab-profile
    revision: master
    path: docs/common/git-interaction-policy.md
    url: https://gitlab.com/smart-assets.io/gitlab-profile/-/blob/master/docs/common/git-interaction-policy.md
  - purpose: current
    repository: https://gitlab.com/smart-assets.io/gitlab-profile
    checkout: ../../SA/top-level-gitlab-profile
    revision: master
    path: docs/common/core-prompt/simplified-technical-english.md
    url: https://gitlab.com/smart-assets.io/gitlab-profile/-/blob/master/docs/common/core-prompt/simplified-technical-english.md
  - purpose: current
    repository: https://gitlab.com/smart-assets.io/gitlab-profile
    checkout: ../../SA/top-level-gitlab-profile
    revision: master
    path: docs/common/cross-reference-standards.md
    url: https://gitlab.com/smart-assets.io/gitlab-profile/-/blob/master/docs/common/cross-reference-standards.md
---

# F1R3node Agent Instructions

<!-- ste-policy: required -->

This repository implements a pure Rust blockchain node with CBC Casper consensus.
Use Cargo and system dependencies. Do not introduce Nix, SBT, or Scala.
`CLAUDE.md` loads this file. Keep shared instructions here and detailed policies in their existing sources.

## Start here

1. Read [ToDos](docs/ToDos.md) and related work logs for task status, claims, and earlier progress.
2. Read the [contribution guide](CONTRIBUTING.md) before changing the repository.
3. Follow the applicable sources in the reference map below.

Follow the user's current instructions when repository guidance conflicts with them.
Trace new policy requirements to user instructions or an approved decision.
Existing code, generated claims, and historical evidence do not establish authorization for a new requirement.

Do only the requested work. Ask for clarification when required information is missing.
Before code changes, identify the affected files, method, and assumptions.
Use a subagent only when the user explicitly requests delegation.

## Git controls

Follow the workspace Git policy declared in the frontmatter before changing repository state.
Interactive mode is the default. A user-created worktree does not select agentic mode.

- Run GitHub CLI commands as `env -u GITHUB_TOKEN gh ...` to use stored credentials.
- Use the harness commit and push commands. Do not directly stage, commit, or push unless explicitly requested.
- Commit consent permits one commit, including merge commits and equivalent plumbing operations.
- Resolve merge conflicts without a merge commit unless the user separately authorizes that commit.
- Work in this checkout. Do not create a worktree without an explicit user request in interactive mode.
- Get confirmation before moving files with Git or removing or pruning worktrees.
- Get confirmation before stash mutations. Do not use stash `pop`, `drop`, `clear`, or `branch`.
- Get confirmation before you create, switch, or delete a branch. Put follow-up work on the current branch unless the user selects another branch.
- Do not commit or move another agent's changes.
- Do not add assistant attribution, emoji, `Co-Authored-By`, or `Claude-Session` trailers. Disable harness co-author attribution.
- Use the `[agent]` commit prefix in explicitly selected agentic mode.
- Let Mergify merge pull requests. Merges into `ca-canary` require no pre-merge soak status or label.
- Open new pull requests with `--label ci-heavy`, unless the documented maintainer waiver applies.

## Development controls

- Use Rust and Bash for new work. Do not add code comments unless the user requests them.
- Keep the pinned shared CbC runtime outside this repository. Keep its launcher, installer, and checks in Rust or Bash.
- Preserve the scoped Python exception for `scripts/bench/extend-issue24-metrics.py` and its test.
- Remove legacy Python when its formal witnesses or older inline scripts next change.
- Never expose credentials, private keys, or personal data in source, examples, logs, or errors.
- Reuse existing JSONC or JSON5 configuration. Prefer JSONC for new configuration when comments help maintenance.
- Keep harness settings personal. Disable checkpoints that run stash or hard reset around tool events.
- Set shell timeouts to at least 20 minutes, with a maximum of 30 minutes, for the pre-push gate.

For React tests, assert user-observable structure, state, and behavior through accessible queries.
Use `data-testid` when no accessible handle exists, or for stable component boundaries.
Assert literal copy or values only when copy, formatting, or content transformation is the requirement.
Avoid display-copy counts and CSS classes as substitutes for behavior assertions.

## Writing controls

Read and follow the STE and cross-reference policies declared in the frontmatter before writing technical prose.
Use Markdown and Mermaid as the canonical artifact format. Generate HTML exports when requested.
Use document-relative links within this repository and structured records for references to other repositories.
Run the STE Check for changed prompt or policy files and the reference checker for changed Markdown files.
An automated STE Check does not replace human STE Review.

## Reference map

| When needed | Existing source |
| --- | --- |
| Branches, commits, security, and contributions | [Contribution guide](CONTRIBUTING.md) |
| Setup, toolchain, hooks, commands, and workspace layout | [Developer guide](DEVELOPER.md) |
| Rust patterns, async work, errors, logging, and serialization | [Patterns](docs/patterns/README.md) |
| Crates and component architecture | [Documentation index](docs/README.md) |
| Canonical terminology | [Glossary](docs/Glossary.md) |
| Commit identities, transfers, CI, waivers, and queues | [CI guide](docs/ci.md) |
| Stories, epics, claims, coordination, and skill names | [Task tracking](docs/task-tracking.md) |
| Correctness claims and evidence | [CbC verification tiers](docs/cbc-verification-tiers.md) |

Load each source when its subject applies. Update the existing source instead of copying its policy into another file.
