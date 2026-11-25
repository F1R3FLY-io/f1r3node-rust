# Hierarchical CbC Attributes Migration Plan (EPIC-026)

- **Status:** Draft, 2026-10-12. Blocked until the maintainer approves the design and the shared tooling lands.
- **Task:** EPIC-026 in `docs/ToDos.md`
- **Branch:** `chore/cbc-hierarchical-attributes`, base `dev`
- **Design owner:** `top-level-gitlab-profile`. Its `docs/ToDos.md` INBOX entry "hierarchical cbc .gitattributes design (from f1r3node-rust, 2026-10-12)" holds the rules R1 to R7, the revisions, and the consumer reply.

## 1. Scope

This repository moves its `cbc` attribute rules out of the root `.gitattributes` file into `.gitattributes` files in owner directories. The effective policy does not change. Every tracked path keeps the same `cbc`, `cbc-weight`, `cbc-acceptance`, and `cbc-evidence` values.

The shared tooling, the design rules, and their tests stay in `top-level-gitlab-profile`. This repository only applies the rules to its own files.

These items are out of scope:

- Per-path evidence routing (R7). It is a separate interface change. The consumer auditors use fixed paths, so this migration does not need it.
- New `cbc-acceptance` rules. No path sets `cbc-acceptance` today.
- Source file moves. A source move changes the path in signed approval records, so it needs fresh verification.
- New tags and changes to tag coverage.

## 2. Baseline

The root `.gitattributes` file has 160 lines. It has no `[attr]` macros and no filter or text rules.

- Every pattern contains a slash. Git therefore anchors each pattern to the root, and no basename rule exists.
- Six patterns are recursive globs (`**`): four under `formal/`, one under `casper/`, and one under `scripts/`.
- Two lines are exact duplicates: `.github/workflows/slashing-tests.yml` (lines 13 and 125) and `scripts/ci/check-tla-invariants.sh` (lines 10 and 117).
- No `.git/info/attributes` file and no `core.attributesFile` setting exist in the reference clone.

At `dev` `c6b5aa5be`, 403 tracked paths have `cbc=mandatory`. Of these, 341 have `cbc-weight=high` and 62 have `cbc-weight=medium`.

The evidence directories are `docs/cbc-evidence/` and `docs/casper/cbc-evidence/`. These consumers read the second directory by a fixed path:

- `scripts/casper-soak/src/bin/check-casper-claims.rs`
- `scripts/ci/check-casper-soak-bindings.sh`
- `scripts/casper-soak/tests/claims.rs` and `scripts/casper-soak/tests/bindings.rs`

## 3. Snapshot command

Until the shared R5 command lands, this command gives the same data for any commit:

```bash
git ls-files -z \
  | git check-attr --stdin -z --source="$REV" cbc cbc-weight cbc-acceptance cbc-evidence \
  | tr '\0' '\n' | paste - - - | awk -F'\t' '$3 != "unspecified"' | LC_ALL=C sort
```

`--source` reads the attribute files from a commit, so a comparison needs no checkout. The command needs git 2.40 or later.

A comparison runs the command for two commits and compares the outputs with `diff`. An empty diff proves that the effective policy did not change. When the R5 command lands, it replaces this command. R5 also reports overrides, duplicates, unanchored patterns, and ambient attribute sources.

## 4. Owner map and path mapping

| Owner directory | Paths with `cbc=mandatory` | New file |
|---|---|---|
| `formal/` | 155 | `formal/.gitattributes` |
| `scripts/` | 104 | `scripts/.gitattributes` |
| `casper/` | 68 | `casper/.gitattributes` |
| `crypto/` | 27 | `crypto/.gitattributes` |
| `rspace++/` | 19 | `rspace++/.gitattributes` |
| `.github/` | 16 | `.github/.gitattributes` |
| `block-storage/` | 8 | `block-storage/.gitattributes` |
| `node/` | 5 | `node/.gitattributes` |
| `shared/` | 4 | `shared/.gitattributes` |
| `rholang/` | 1 | `rholang/.gitattributes` |
| `docs/` | 1 | `docs/.gitattributes` |
| Root | 0 | `.gitattributes` keeps no `cbc` rule |

The counts are effective paths, not lines, from the 2026-10-22 baseline. A glob line covers more than one path.

The root file keeps no `cbc` rule. The shared lint reports `potential_override` when a child file and an ancestor file set the same attribute names, also when their patterns do not overlap. With no `cbc` rule in the root file, no owner file has a `cbc` ancestor, and the lint reports no override.

The mapping rule is the same for every line. Remove the owner prefix, and then add a leading slash. The attributes stay the same.

| Root line | Owner file line |
|---|---|
| `casper/src/rust/validate.rs cbc=mandatory cbc-weight=high` | `/src/rust/validate.rs cbc=mandatory cbc-weight=high` in `casper/.gitattributes` |
| `casper/src/rust/finality/** cbc=mandatory cbc-weight=high` | `/src/rust/finality/** cbc=mandatory cbc-weight=high` in `casper/.gitattributes` |
| `.github/workflows/merge-recovery-soak.yml cbc=mandatory cbc-weight=high` | `/workflows/merge-recovery-soak.yml cbc=mandatory cbc-weight=high` in `.github/.gitattributes` |

The leading slash keeps each rule anchored as it is today. Without it, a pattern without a slash would match at any depth below the owner directory.

## 5. Steps

1. **Record the baseline.** Run the snapshot command at the branch base. Keep the output with the pull request as the reference.
2. **Declare the owners.** Each owner directory of section 4 gets its own `.gitattributes` file in step 3. The shared tooling has no repository-local owner list. Without a declared owner, R1 routes a proposal to the nearest ancestor that has a `.gitattributes` file. The owner files therefore route new proposals, and this repository adds no owner configuration. The routing does not change the effective policy.
3. **Move one owner directory in each commit.** Create the owner file and remove the matching root lines in the same commit. After each commit, run the snapshot comparison against the previous commit. The diff must be empty.
4. **Remove the duplicate lines in a separate commit.** Step 3 moves both copies of each duplicate line unchanged. A later commit removes the second copy. A duplicate line has no effect on the effective policy, so the snapshot diff must be empty.
5. **Run the R5 warnings.** After the last move, run the shared R5 command. It must report no override, no duplicate, no unanchored single-file pattern, and no ambient attribute source.
6. **Check the gate.** Run `cbc discharge --strict` at the branch base and at the branch head. Both runs must give the same result.

## 6. Verification

The migration is complete when all these conditions are true:

- The snapshot at the branch head equals the baseline, except for paths that `dev` changed during the work. Each such path is recorded with its commit.
- Each owner file uses only anchored patterns and no `[attr]` macro.
- The root `.gitattributes` file holds only the two root paths of section 4.
- The R5 warnings are clean.
- `cbc discharge --strict` gives the same result before and after the migration.
- The independent verifier in `top-level-gitlab-profile` confirms the result.

## 7. Rollback

Each step 3 commit moves one owner directory and nothing else. A revert of one commit restores the root lines for that directory. The snapshot comparison then shows an empty diff again.

The migration changes no evidence record and no acceptance record. A rollback therefore needs no evidence change.

## 8. Conflicts with other branches

After the migration merges, a new tag goes in the owner file, not in the root file. A branch that edits root `.gitattributes` lines must move its edits to the owner file during its next merge with `dev`.

On 2026-10-12, one open branch changes `.gitattributes`. `feature/consensus-runtime-cordial` renames three `node/src/rust/instances/` paths to `node/src/rust/consensus/casper/instances/`. That branch moves source files, so it also needs fresh verification and acceptance for those paths. Its rename goes into `node/.gitattributes` with the leading-slash form.

Run the snapshot command on each merge result. An unexpected diff shows a lost or duplicated rule.
