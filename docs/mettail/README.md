# MeTTaIL: the Theory surface and the elaborator

This directory documents the MeTTaIL DDL and module system as frozen on
19 August 2026, and the branch that carries it.

| Document | What it is |
|----------|------------|
| `mettail-ddl-and-modules-2026-08-19.md` | The normative plan. Decisions D1–D10, the surface in §3, the six additions G1–G6 in §3.4, phases in §5, open items in §9. |
| `mettail-for-developers.pdf` | The SQL-to-rholang exposition for a working software developer. |
| `mettail-for-developers-source.tar.gz` | Source for the above. |

## What is on this branch

The branch adds the Theory/module surface to the `feature/module-syntax`
line and nothing else. It does **not** carry the fileio or native
cost-accounting work: `phloPrice` and `phloLimit` remain live in
`DeployDataProto`, so nothing propagates to the Embers HTTP API, the
`@f1r3fly-io/embers-client-sdk` package, or F1R3Sky.

| Piece | Where |
|-------|-------|
| Elaborator | `mettail-elab/` (promoted from `module-syntax/`) |
| Module corpus | `mettail-elab/examples/modules/` |
| Tree-sitter grammar delta | applied on the `rholang-rs` side; see below |
| Pre-flight module checker | `scripts/check-modules.py` |
| Toolchain pin for the crate | `mettail-elab/rust-toolchain.toml` (stable) |

## The elaborator is a client library

D7 says elaboration happens at compile time, and D6 says imports resolve by
URL at the client. So the elaborator is not primarily a node component — the
client resolves, elaborates, and deploys the result. In the Plausible Fiction
prototype **Embers is that client**.

Two design consequences are load-bearing and should not be undone casually:

1. **`mettail-elab` declares its own `[workspace]`.** It is not a member of
   the root workspace. That is what lets Embers take it as a git dependency
   without dragging in the node workspace, and it is why the CI job in
   `.github/workflows/mettail-elab.yml` exists separately.
2. **It has no dependencies.** The CI job asserts this. The crate is on
   somebody else's compile path, and a transitive dependency graph acquired
   here shows up in the Embers build.
3. **It pins `stable`, not the root's `nightly-2026-02-09`.** rustup walks
   upward from the working directory, so without `mettail-elab/rust-toolchain.toml`
   a build in this directory silently inherits the node's nightly — and so
   would Embers. The crate builds and tests clean on rustc 1.75.

### Consuming it from Embers

```toml
mettail-elab = { git = "https://github.com/F1R3FLY-io/f1r3node-rust.git", rev = "<this branch>" }
```

The public surface is small:

```rust
use mettail_elab::{elaborate, Presentation, DiagKind};
use mettail_elab::resolve::{Resolver, MemResolver, FileResolver};

pub fn elaborate(entry_url: &str, r: &dyn Resolver) -> Result<Presentation, Diag>;
```

`Resolver` is the seam. `FileResolver` is for local development and
`MemResolver` for tests; Embers stores module sources in Postgres rather than
on disk, so it wants `MemResolver` (or its own impl over the same trait) —
loading the theory text for a given version and handing it in. The trait's
`join` already handles relative references against an importing document.

`Program::lockfile()` records the resolved import graph. Plan §9.1 is open
and visible here: a bare URL is not a reproducible reference, and until a
content hash joins the surface, the lockfile is what a build has to record.

## Rebuilding the parser side

The grammar delta lives in `rholang-rs`, not here. It is applied on a branch
cut from `feature/cost-accounting-transpiler` (`02cef80`), which is public and
already carries the cost-accounted surface syntax (`signed_term`,
`token_stack`, `signature`) and `where` guards on match cases, receipts and
joins.

The patch applies cleanly to `02cef80` — one hunk, at a 65-line offset. What
does *not* come free is the code around it: `rholang-parser/src/ast.rs` and
the node's `cost_accounting` normalizer were written against the older AST,
and `02cef80` merged master twice (25 July, 8 August). Expect the compile
breakage to be there.

`rholang/Cargo.toml` must be repointed at the resulting rev, and the root
`Cargo.toml` must **not** carry the old `[patch]` stanza pointing at
`../rholang-rs-cost-accounting-transpiler/` — a relative path that exists on
exactly one machine.

## The Plausible Fiction corpus

`examples/modules/PFLam.module` and `PricedPFLam.module` are the object
language of the two papers in `publications/plausible-fiction/`, revision of
26 August 2026, as elaborable modules rather than listings.

`PFLam.module` decomposes the calculus into `Core`, `Records`, `Variants` and
`Holes`, joined over one `Core` instance. That is not presentation: it makes
the join a pushout, so the corpus test can assert that a `Term` carrier
appears once rather than three times — the same property `Rholang.module`
demonstrates over `QuoteDropCalc`.

`PricedPFLam.module` extends `pf.PFLam` across a module boundary, which is
what makes the pricing delta *checkable*. Under the withdrawn `language!`
form it could only be a listing annotated "only additions shown".

Three things are deliberately absent from both, per §4.3 of the revised
paper: `grounded`/`holes`/`plug` (host operations at the bridge — `holes`
does not even return a `Term`), the `logic { relation … }` block (the
bidirectional checker is an ascent program beside the theory), and native
ground-type aliases such as `![String] as HoleId` (plan §9.4 is open, so
`HoleId`, `MetaId`, `Tag`, `Lvl`, `Price` and `Obsv` are declared as ordinary
categories with no constructors, exactly as `Elem` is in `EmptySet()`).

### One correction to file against the papers

Both revised papers say `HashBag` is the only collection sort the frozen
surface offers. That is true of the *examples*, but `parse.rs::sort` accepts
`HashBag`, `Set` and `List`. The substantive point survives and should be
restated more precisely: none of the three is **keyed**, so two alternatives
sharing a `Tag` but differing in type are distinct elements of a `Set` as
readily as of a `HashBag`. Label-uniqueness across variant alternatives and
case arms is therefore still a side condition rather than a structural
guarantee. That is plan §9.3, and PFLam is its first real consumer.
