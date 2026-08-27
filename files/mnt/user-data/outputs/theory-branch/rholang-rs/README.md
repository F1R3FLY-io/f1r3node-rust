# The parser side

The Theory/Module surface needs a grammar delta in `rholang-rs`. It is not
part of the f1r3node-rust branch; it is a separate branch there, and the node
pins its rev.

## Construction

```bash
git clone https://github.com/F1R3FLY-io/rholang-rs.git
cd rholang-rs
git checkout -b feature/mettail-ddl c163755
git apply ../0001-mettail-ddl-grammar.patch
```

`c163755` is the `feat/where-clauses` merge of 10 May 2026. It is public, it
is reachable from `master`, and it is **already the rev the node pins** at
base `91b5c70a` — so applying the delta here means the node's pin moves by
one commit rather than jumping across three months of parser history.

The patch applies cleanly at `c163755`: two hunks, offsets of -9 and -128
lines, no conflicts. Verified 26 August 2026.

### The other option, and why not to take it yet

`feature/cost-accounting-transpiler` (`02cef80`) also exists, is also public,
and the patch also applies there. It additionally carries the cost-accounted
surface syntax — `signed_term`, `token_stack`, `signature` with `-o` — which
`c163755` does not.

Take it only when something needs that surface. The Plausible Fiction
prototype does not: the compile path is Embers-side elaboration producing
ordinary rholang. And `02cef80` merged master twice after the delta was
written (25 July, 8 August), so `ast.rs` and the node's `cost_accounting`
normalizer are the likely site of breakage there. `c163755` has no such gap:
it is contemporaneous with the delta.

`where` guards are present at `c163755` — that commit is the merge that
added them, and it is what the node's `where`-clause support is built on.

## What the patch does and does not do

It is a 187-line delta to `rholang-tree-sitter/grammar.js` only. It adds the
`module`, `import`, `theory_decl` and `theory_inst` productions, the five
builder blocks, and the judgement-form term-rule sublanguage.

It does **not** carry:

- regenerated tree-sitter tables (`rholang-tree-sitter/src/`)
- the new AST nodes in `rholang-parser/src/ast.rs`
- the node-to-AST mapping in `rholang-parser/src/parser/`
- round-trip tests

That is plan §4.1, and it is the real work. Budget for it accordingly.

## The likely source of breakage

The delta was written against `c163755` plus a local worktree at `51e28a6`,
so at `c163755` the grammar hunks land on contemporaneous code. What is still
missing is everything downstream of the grammar: the regenerated tables, the
AST nodes, and the node-to-AST mapping. If something fails, that is where.

## Keyword handling

Per plan §4.1: `Module`, `Theory`, `Exports`, `Replacements`, `Terms`,
`Equations`, `Rewrites`, `import`, `as`, `from`, `free` must **not** join the
global reserved set. Follow the precedent documented in the grammar preamble
for `agent`/`constructor`/`method`/`default`/`private` and let the GLR parser
disambiguate by context. Reserving them globally breaks existing Rholang that
uses these as identifiers.

## Gate

The corpus round-trips; the pin is bumped in f1r3node-rust; the existing
Rholang test suite is unaffected.
