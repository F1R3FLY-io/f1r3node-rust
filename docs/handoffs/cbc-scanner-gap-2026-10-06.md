---
doc_type: handoff
from_repo: f1r3node-rust
to_repo: top-level-gitlab-profile
created: 2026-10-06
handoff_status: ready
relevance: [TASK-021-11]
next_steps:
  - Choose a fix for the gap below in the profile repository, in its own session.
  - Add a scanner test that covers the chosen fix.
---

# Handoff: `/cbc:review` misses state-root code

## Finding

On 2026-10-06, `/cbc:review --base origin/dev` on `fix/issue-24-deepening-resolution` (PR #653) found no candidates. The branch changes the rspace history code that computes and stores the state root. The state root is consensus data.

The scanner `AItools/scripts/cbc-identify-critical.sh` uses `AItools/scripts/lib/cbc-critical-patterns.jsonc`. Its rules match only `consensus`, `crypto`, `contracts`, `auth`, `payments`, `keystore`, `.sol`, and `.move`. No rule matches `rspace++/src/rspace/history` or `shared/src/rust/store`.

The scanner reads `--config` and `CBC_CRITICAL_CONFIG` only. It does not read a configuration file from the scanned repository, and the profile documents no path for one.

## Action in f1r3node-rust

The user accepted nine `cbc=mandatory` tags in `.gitattributes` by manual review. The tagged files are in CbC scope now, so this repository needs no scanner change.

A temporary repository file `.cbc/critical-patterns.jsonc` showed that two rules find all nine files. They are a `dir` rule for `history` (risk 3) and a `dir` rule for `store` (risk 2). The user removed that file because it is not an established pattern.

## Options for the profile repository

1. Read an optional repository configuration from a documented path, and merge it with the default rules.
2. Add a default rule for state and storage code, for example the `dir` names `history` and `store`. This can propose more candidates in other repositories.
