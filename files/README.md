# Theory-syntax application branch — bundle

Everything needed to construct an f1r3node-rust branch that supports the
Plausible Fiction prototype. Nothing here is pushed anywhere; run
`make-branch.sh` against a clone and review before committing.

```
make-branch.sh                     construct the branch from a clone
check-modules.py                   pre-flight checker for .module files
files/                             what gets dropped into the branch
  mettail-elab/examples/modules/   PFLam, PricedPFLam, one G6 negative
  mettail-elab/tests/pflam.rs      corpus tests for the above
  .github/workflows/               CI for the standalone elaborator crate
  docs/mettail/README.md           branch orientation + the Embers seam
rholang-rs/                        the parser-side branch, separately
  0001-mettail-ddl-grammar.patch   the DDL grammar delta
  README.md                        how to build it and what it omits
```

## Usage

Either let the script make the branch:

```bash
./make-branch.sh /path/to/f1r3node-rust app/plausible-fiction-prototype
```

or make it yourself first and pass the same name:

```bash
cd /path/to/f1r3node-rust
git checkout -b app/plausible-fiction-prototype 91b5c70a
cd /path/to/bundle
./make-branch.sh /path/to/f1r3node-rust app/plausible-fiction-prototype
```

Then:

```bash
cd /path/to/f1r3node-rust/mettail-elab && cargo test
```

The script refuses a dirty tree, refuses a branch that exists but is not at
the base, and refuses `feature/mettail` (taken by unrelated RSpace work). It
cherry-picks, moves, installs, pre-flights, and stages — but does not commit.

## What the branch is

Base **`91b5c70a`** — the where-clauses-and-match-guards merge of 21 May
2026 — plus a cherry-pick of `9c57a82b` (the module-syntax work) and four
changes:

1. `module-syntax/mettail-elab/` promoted to a top-level `mettail-elab/`,
   keeping its own `[workspace]` so Embers can take it as a git dependency,
   and pinned to `stable` so it does not inherit the root nightly.
2. The two papers' object language added as elaborable modules, with tests.
3. CI for the elaborator, which the root workspace job does not cover.
4. The grammar delta parked at `rholang-rs/mettail-ddl.patch` and the
   pre-flight checker at `scripts/check-modules.py`.

## Why this base rather than `feature/module-syntax`

`feature/module-syntax` is `91b5c70a` → `0f6ee989` → `9c57a82b`. The middle
commit is the problem. It rewires the root `Cargo.toml` with

```toml
rholang-parser = { path = "../rholang-rs-cost-accounting-transpiler/rholang-parser" }
```

plus a matching `[patch]` stanza — a sibling directory that exists on exactly
one machine. Branching from `91b5c70a` and picking `9c57a82b` over it means
never having to remove them, and the pick is clean: 24 files, all new, all
under `module-syntax/`.

Basing here also buys:

- **Exact parity with the demoed node.** `91b5c70a` is the rev
  `Embers@dylon/embers-demo-fixes` pins for `f1r3node-models`. The branch
  starts at the commit the SF and August demos ran against.
- **A public parser pin.** `rholang/Cargo.toml` resolves `rholang-parser`
  from `git rev = "c163755"`. Clonable and buildable by anyone.
- **`where` guards.** `91b5c70a` *is* the where-clauses merge, and `c163755`
  is the parser merge that matches it.
- **No retired code.** `0f6ee989` carries the source-to-source cost-accounting
  transpiler, whose Part B (`lower.rs`, `infra.rs`, `oslf.rs`) has since been
  explicitly dropped on the cost-accounted line. The prototype does not need
  it, and inheriting a known-dead path costs something later.

## Embers-side changes this branch expects

Branch off `dylon/embers-demo-fixes` — not `main` (Scala node, vendored
protos) and not `feat/rust-node-compatibility` (March, wrong repo,
superseded).

```toml
# packages/embers/Cargo.toml
mettail-elab = { git = "https://github.com/F1R3FLY-io/f1r3node-rust.git", rev = "<branch>" }
```

`f1r3node-models` can stay at `91b5c70a`, or be bumped for traceability.
Either is correct.

The compile path becomes: GraphL → Theory/module text → `mettail_elab::elaborate`
→ `Presentation` → rholang → the existing askama render → `prepare_for_signing`.
No proto change, no signing change, no SDK bump. F1R3Sky is untouched.

## Verification status

Built and tested. `apt-get install cargo rustc rustfmt` gives rustc 1.75.0
from the allowed mirror; the crate is dependency-free and edition 2021, so it
builds on it.

```
   tests/corpus.rs   19 passed; 0 failed
   tests/pflam.rs     6 passed; 0 failed
```

The branch construction itself was dry-run end to end against a clone at
`91b5c70a`: cherry-pick clean, 31 files staged, `module-syntax/` gone, and
`cargo test` green from inside `mettail-elab/` in the constructed tree.

All six new tests pass, and the hand-derived counts in them were right: the
three-way join over one `Core` yields 7 categories and 16 labels, and
`PricedPFLam` yields 9. `tests/pflam.rs` is rustfmt-clean.

Both modules elaborate. `cargo run -- examples/modules/PFLam.module` prints
the presentation the revised paper specifies: one `Term` carrier, `Case`
with its motive spelled (`"case" scrut "return" mot "of" ...`), variant beta
as a remainder pattern with the repeated label `l`, the eta side condition as
`if x # f then`, and no `Grounded`, `HolesOf`, `Plug`, `SelectArm` or `Var`
among the constructors.

`PricedPFLam.module` elaborates across the module boundary and carries the
base theory with it — `Priced`, `Scale`, `Give`, `ObsOne` arrive alongside
all sixteen PFLam constructors, and `(Give (Give c)) == c` sits in the same
equation set as beta. No `Val`.

The negative produces the diagnostic it should:

```
CaseMotiveUnused.module: 30:7: [argument-use] term `Case` binds `mot` but
references it 0 times in its concrete syntax; each argument must appear
exactly once
```

**One thing running it found.** The repository root pins
`nightly-2026-02-09` in `rust-toolchain.toml`, and rustup walks upward from
the working directory — so a build inside `mettail-elab/` would inherit that
nightly even though the crate does not need it, and the CI job's
`dtolnay/rust-toolchain@stable` would be overridden. The bundle now ships
`files/mettail-elab/rust-toolchain.toml` pinning stable. Embers takes this
crate as a git dependency and should not be dragged onto the node's nightly.

**Still not verified:**

- Anything on the parser side beyond "the patch applies." No tree-sitter
  regeneration, no `ast.rs` work, no round-trip tests — that is plan §4.1 and
  it is the real remaining work.
- The rest of the f1r3node-rust workspace, which does want the nightly and a
  much longer build.
- `cargo clippy` on the elaborator: the distro toolchain ships neither clippy
  nor `cargo fmt` as subcommands. `rustfmt --check` passes on the new file;
  the CI job runs both properly.

`check-modules.py` still ships. It is now redundant where cargo is available,
but it is fast, needs no toolchain, and is what the CI job runs over the
corpus before the Rust tests.

## One correction to file against the papers

Both revised papers say `HashBag` is the only collection sort the frozen
surface offers. `parse.rs::sort` in fact accepts `HashBag`, `Set` and `List`.
The substantive point survives but should be restated: none of the three is
**keyed**, so two variant alternatives sharing a `Tag` but differing in type
are distinct elements of a `Set` as readily as of a `HashBag`.
Label-uniqueness is therefore still a side condition rather than a structural
guarantee — plan §9.3, with PFLam as its first real consumer. Say the word
and I will re-cut both PDFs with the corrected wording.
