---
title: MeTTaIL DDL and module system — landing the contemplated syntax
status: draft
author: claude-session
date: 2026-08-19
related-docs:
  - docs/plans/where-clauses-and-match-guards-2026-04-29.md
  - docs/cost-accounting/transpiler.md
  - docs/rholang/02-syntax-reference.md
related-repos:
  - F1R3FLY-io/MeTTaIL (branch `dev`) — the normative module grammar and Scala elaborator
  - F1R3FLY-io/rholang-rs — the live tree-sitter parser
  - F1R3FLY-io/publications — `MeTTaIL4WorkingDev/`, `GSLT-intro/`
---

# MeTTaIL DDL and module system

## 1. Situation

Rholang 1.4 adds a data definition language to Rholang 1.2 and refines 1.2's
control flow language. The refined CFL has landed on
`feature/cost-accounting-transpiler`: per-group `where` guards on receives,
`where` guards on match cases with fall-through, and the cost-accounting
surface (`s :: ()`, `{% P %}[s]`, `(*)`, `# P`). The lollipop is out of scope;
see D8.

The DDL has not landed. Neither has the module system. Three surfaces
currently disagree:

| Source | Term notation | Module system | `where` |
|--------|---------------|---------------|---------|
| `MeTTaIL@dev`, `GSLT/src/main/bnfc/metta_venus.cf` | `Label . Cat ::= Items` | full `Module`/`Theory`, imports, functors | absent (`LinearCond` commented out) |
| `f1r3node-rust@feature/cost-accounting-transpiler` | none | none | present, per group |
| `publications/MeTTaIL4WorkingDev`, part2 | `Label . ctx \|- syntax : Cat` | none | one trailing `where` |

The documentation therefore describes a fourth thing: neither what runs nor
what is contemplated. Worse than a lag — the `language!` block as documented
has no parameters, exports, replacements, or imports, so it cannot express the
`UnivAlg -> ParMonoid -> RhoCalc` construction that is the entire argument for
having a theory algebra at all.

This plan resolves the three surfaces into one, implements it, and brings the
documentation onto the result.

## 2. Decisions taken

Recorded 2026-08-19. These are settled; §9 lists what remains open.

**D1. One surface.** The `language!` specification is normative for semantics
and for the notation of term, equation, and rewrite content. The
`Module`/`Theory` packaging from `metta_venus.cf` is normative for reuse.
Superseded in part by D10.

**D2. Judgement-style terms win.** `Label . ctx |- concrete-syntax : Cat`
supersedes the BNFC `Label . Cat ::= Items` form. The judgement form carries
explicitly sorted argument contexts, higher-order binder sorts
(`^x.p:[Name -> Proc]`), collection sorts with separator syntax
(`ps:HashBag(Proc)` rendered `ps.*sep("|")`), remainder patterns (`...rest`),
and conditional rewrites in `if S ~> T then ...` form.

**D3. The theory algebra is a lattice of presentations, and `\/` is a
pushout.** `\/` is join, `/\` meet, `\` difference, ordered by inclusion of
presentations. When two theories descend from a common parameter — as
`NewReplCalc` and `RhoCalc` both descend from `ParMonoid` — their join is taken
*over* that shared parameter. `Rholang(nr, rc) { nr \/ rc }` yields one `PPar`,
not two.

**D4. Space goes away.** `Space` declarations, the `SpaceInst` algebra
(`NilSpace`, `::`, `<|`, `**`, `tail`/`sup`/`inf`, the `=> { … }` continuation
form) and the `FactComprehension` layer are all struck. Spaces are channels.
The CFL is Rholang's existing `for`/`!` over channels, already implemented.

**D5. `where` is per group.** `for( binds_1 where cond_1 ; … ; binds_k where
cond_k )P`, each `cond_j` referencing variables bound in groups `i <= j`, the
receive committing only when every pattern matches and every condition holds.
This matches the implementation and settles the one-trailing-`where` form used
in the papers against it.

**D6. Imports resolve by URL.** `import "<url>" as u`. Aligns with on-chain
addressing.

**D7. Elaboration is compile time.** A client toolchain resolves imports,
elaborates the theory expression to a presentation, and generates the parser,
printer, substitution, and rewrite engine. The node receives elaborated
artifacts. Elaboration is not metered, because it does not run on the node.

**D8. The lollipop does not go into this implementation.** `-o` delegation is
excluded from the delivered surface. Nothing in §3–§7 depends on it. It does
have a downstream consequence outside this plan: `examples/cost_accounting_demo.rho`
currently ships a `-o` scene (Cy's delegated buy, principal funding the order
rendezvous and agent the settlement) and `docs/cost-accounting/demo.md`
documents it in the feature index. Both need amending, and that work is tracked
as 9.5 rather than as a phase here.

**D9. Symmetric receipts are rejected as a feature.** `<->`, `<=>`, `<<->>`
leave the grammar. `!$` (`SendSymm`) goes with them — see 9.2.

**D10. `language!` is withdrawn.** `language! { name: T, … }` is replaced by the
equivalent `Theory T() { … }`. There is one declaration form, with parameters
optional, rather than two forms differing only in whether reuse is permitted.
The semantics and notation D1 imported from `language!` are unaffected; what
goes is the second syntactic wrapper. §3.4 enumerates the elements the `Theory`
grammar must gain to absorb it.

## 3. The surface, normatively

### 3.1 One form only — `Theory`

`language!` is withdrawn. `language! { name: T, … }` becomes `Theory T() { … }`,
which is the same declaration with the parametric packaging available rather
than forbidden. The running example, in the surviving form:

```
Theory RhoCalc() {
    Types { Proc; Name; }
    Terms {
        PZero  . |- "0" : Proc;
        PDrop  . n:Name |- "*" "(" n ")" : Proc;
        POutput. n:Name, q:Proc |- n "!" "(" q ")" : Proc;
        PInput . n:Name, ^x.p:[Name -> Proc] |- n "?" x "." "{" p "}" : Proc;
        PPar   . ps:HashBag(Proc) |- "{" ps.*sep("|") "}" : Proc;
        NQuote . p:Proc |- "@" "(" p ")" : Name;
    }
    Equations {
        (NQuote (PDrop N)) == N;
    }
    Rewrites {
        RComm : (PPar {(PInput n ^x.p), (POutput n q), ...rest})
                  ~> (PPar {(subst ^x.p (NQuote q)), ...rest});
        RDrop : (PDrop (NQuote P)) ~> P;
        RPar  : if S ~> T then (PPar {S, ...rest}) ~> (PPar {T, ...rest});
    }
}
```

Four differences from the withdrawn block are visible above, and all four are
the `Theory` form asserting itself: `name:` is carried by the declaration head;
`types`/`terms`/`equations`/`rewrites` become the capitalised builders and lose
their comma separators; `=` becomes `==`; and rewrite names become mandatory,
because `\` cannot subtract, and `Replacements` cannot retarget, an anonymous
rule.

Section 3.4 covers what is *not* visible above: the elements of the withdrawn
spec that the `Theory` grammar does not yet have, and must gain.

### 3.2 Packaged — `Module` and `Theory`

```
import "<url>" as u                    -- whole module under an alias
import Monoid from "<url>"             -- one theory, unqualified

Module Rholang {

  Theory ParMonoid(cm: u.CommutativeMonoid) {
    cm
      Exports { Elem => Proc; }
      Replacements {
        Zero => PZero . |- "0" : Proc;
        Plus => PPar  . ps:HashBag(Proc) |- "{" ps.*sep("|") "}" : Proc;
      }
      Rewrites {
        RPar : if S ~> T then (PPar {S, ...rest}) ~> (PPar {T, ...rest});
      }
  }

  Theory QuoteDropCalc(pm: ParMonoid) {
    pm
      Exports { Name; }
      Terms {
        PDrop  . n:Name |- "*" "(" n ")" : Proc;
        NQuote . p:Proc |- "@" "(" p ")" : Name;
      }
      Equations {
        (NQuote (PDrop N)) == N;
        (PDrop (NQuote P)) == P;
      }
  }

  Theory Rholang(nr: NewReplCalc, rc: RhoCalc) { nr \/ rc }

  theory FreeRholang()                 -- the entry point to elaborate
}
```

**Structure.** A `Theory` declaration is a parameterised module whose
parameters are typed by other theories. Its body is a theory expression:

- *atoms* — `Empty`, `free(Path)`, a parameter reference, an application
  `QuoteDropCalc(pm)`, `let x = e in (e)`
- *postfix builders*, chainable — `Types`, `Exports`, `Replacements`, `Terms`,
  `Equations`, `Rewrites`
- *combinators*, tightest to loosest — `/\`, `\/`, `\`

Cross-module reference is by dotted path. The module body may hold theory
declarations, theory instantiations, and ordinary `Proc`s. The **last**
`theory …` in the entry module is what gets elaborated.

**The builder chain is ordered, and the withdrawn spec was not.** `language!`
was a fixed record of five fields; a theory expression is a left-to-right chain
in which each builder applies to the result of the one before. That is strictly
more expressive — builders may repeat and interleave, as `AbelianGroup` already
does with `Replacements { … } \/ c` — but it introduces an ordering obligation
the record form did not have: a `Terms` block must precede any `Equations` or
`Rewrites` block mentioning its labels. This is a validation rule, listed in §7.

**`Replacements` lose their integer profile.** In the BNFC form,
`[0,1] Mult . Elem => Plus . Elem ::= …` needed a positional profile to permute
arguments. Under D2 the arguments are named, so permutation is expressed by
naming them in the new order and the profile is dropped.

### 3.3 What is struck

Deleted from `metta_venus.cf` wholesale: `SpaceDecl`, `SpaceInst` and its
sixteen productions, `Fact`, `FactComprehension`, `SynchSendCont` as reached
from facts, and `BasePres`'s space-facing entries.

Also struck, per D9: the symmetric receipt forms `ReceiptSymmLinear` (`<->`),
`ReceiptSymmRepeated` (`<=>`), and `ReceiptSymmPeek` (`<<->>`), together with
their `LinearBindSymm`/`RepeatedBindSymm`/`PeekBindSymm` productions. The
`Receipt` hierarchy is therefore retained only in the three forms Rholang
already implements: `<-`, `<=`, `<<-`. The synchronous sources `x?!` and
`x!?(…)` are unaffected — they are the synch send/receive pairing, not the
symmetric one.

### 3.4 What the `Theory` form must gain

The withdrawn spec is not a strict subset of the `Theory` grammar. Six elements
have no counterpart in `metta_venus.cf` and must be added before D10 can be
honoured. Four are load-bearing; two are ergonomic.

**G1. `Types` — category declaration, distinct from `Exports`.** *Load-bearing.*
The `Theory` grammar has no way to declare a syntactic category. Categories
come into being implicitly, as the result sorts of term rules, and `Exports`
selects among them. That works only because `Exports` has been quietly doing
declaration duty: `EmptySet` in `UnivAlg.module` is `Empty Exports { Elem; }`
with no `Terms` at all, so `Elem` exists solely by being exported. Conflating
the two roles means a theory cannot have a category that is real but private.
Add a `Types { … }` builder; `Exports` reverts to pure visibility-with-rename,
and defaults to everything `Types` declares when omitted.

**G2. Collection sorts.** *Load-bearing.* `ps:HashBag(Proc)` has no counterpart.
The BNFC side offered `[Cat]` — an ordered list — plus `Product { … }` and the
`separator`/`terminator` pragmas. A bag is neither: unordered, with
multiplicity, and it is what makes `PPar` a parallel composition rather than a
sequence. The inline rendering `ps.*sep("|")` also replaces the grammar-level
`separator` pragma, and is strictly better, being per-occurrence rather than
per-category. What must be decided is the full sort vocabulary — bag, set,
list, product, map — since the withdrawn spec exercises exactly one of them and
the form it replaces exercised different ones. See 9.3.

**G3. Remainder patterns in AST position.** *Load-bearing.* `Theory`'s AST
grammar is three formers — variable, s-expression, `Subst` — with no `...rest`.
Without it the COMM rule cannot be written at all: matching an input against an
output inside a `PPar` requires binding the unmatched remainder and
reconstituting it on the right. Every collection-valued rewrite needs this.

**G4. Abstractions as first-class ASTs, and a two-argument `subst`.**
*Load-bearing.* `Theory` has `(Subst body arg var)` — three positional
arguments, with the binder named separately. The withdrawn spec has
`(subst ^x.p (NQuote q))` — two arguments, because `^x.p` is itself an AST and
carries its own binder. The two-argument form is the better one: it cannot be
malformed by naming a variable that does not bind in the body. Adopting it
requires `^x.p` to be an AST former, which it currently is not. Note that the
term-level binder sort `^x.p:[Name -> Proc]` and the AST-level abstraction are
the same construct appearing in two positions, and should be one production.

**G5. An implicit `Empty`.** *Ergonomic.* Builders are postfix on a theory
expression, so a body must begin with an atom. `Theory RhoCalc() { Types { … } … }`
as written in §3.1 has no base and does not parse against the current grammar;
it would have to read `Theory RhoCalc() { Empty Types { … } … }`. Since the
non-parametric case is now the common one, let a body that opens with a builder
take `Empty` as its implicit base.

**G6. Concrete syntax by argument reference.** *Ergonomic, but pervasive.* In
the BNFC form a rule's right-hand side interleaved string terminals with
category nonterminals, positionally. In the judgement form it interleaves
string terminals with *argument names* drawn from the context to the left of
`|-`, and with projections over them (`ps.*sep("|")`). The `Item` vocabulary is
therefore different in kind, not merely in spelling, and the change carries a
new well-formedness rule: every argument in the context must be referenced
exactly once in the concrete syntax. This is the single largest piece of Phase
1 grammar work.

**Not gaps.** Three elements of the withdrawn spec are already present, and
more richly: equations exist and additionally admit a freshness side condition
(`if x # Q then …`), which `language!` had no way to express; conditional
rewrites exist as `let Src ~> Tgt in …` and need only be respelled `if … then
…` per D2; and `name:` is subsumed by the declaration head. In the other
direction, anonymous rewrites are a `language!` element that deliberately does
*not* survive, for the reason given in §3.1.

## 4. Touchpoints

### 4.1 `rholang-rs` (the parser)

| File | Change |
|------|--------|
| `rholang-tree-sitter/grammar.js` | Add `module`, `import`, `theory_decl`, the `theory_inst` precedence chain, the five builder blocks, `language_block`, and the judgement-form term-rule sublanguage. |
| `rholang-tree-sitter/grammar.js` | The six additions of §3.4: `Types` builder (G1), collection sorts (G2), `...rest` remainder patterns (G3), `^x.p` as an AST former with two-argument `subst` (G4), implicit `Empty` (G5), argument-reference concrete syntax (G6). G6 is the bulk of it. |
| `rholang-tree-sitter/src/` | Regenerated tables. |
| `rholang-parser/src/ast.rs` | New AST nodes for the above. |
| `rholang-parser/src/parser/` | Node-to-AST mapping. |
| `rholang-parser/tests/` | Round-trip tests over the corpus in §7. |

**Keyword handling.** `Module`, `Theory`, `Exports`, `Replacements`, `Terms`,
`Equations`, `Rewrites`, `import`, `as`, `from`, `free` must *not* join the
global reserved set. Follow the precedent already documented in the grammar
preamble for `agent`/`constructor`/`method`/`default`/`private`: let the GLR
parser disambiguate by context. Reserving them globally breaks existing
Rholang that uses these as identifiers.

### 4.2 `f1r3node-rust`

| File | Change |
|------|--------|
| `Cargo.toml` (root) | Remove the `[patch."https://github.com/F1R3FLY-io/rholang-rs"]` block; see §5 Phase 0.5. |
| `rholang/Cargo.toml:46` | Bump the `rholang-parser` rev pin. |
| **NEW crate `mettail-elab`** | The elaborator. See §4.3. |
| `rholang/src/rust/interpreter/compiler/normalizer/cost_accounting/oslf.rs` | Accept a presentation; generate the ground formulae and spatial connectives a `where` clause may mention. |
| `rholang/src/rust/interpreter/compiler/normalize.rs` | Widen `is_pure_boolean_expr_par` to admit generated formulae, not only pure boolean processes. |
| `rholang/src/main/bnfc/rholang_mercury.cf` | Reference grammar update, or an explicit note that it is superseded by the tree-sitter grammar. It is currently 205 lines and describes neither `where` nor cost accounting. |
| `docs/mettail/` | New reference documentation. See §6. |

### 4.3 New crate `mettail-elab`

Port of the Scala elaborator, module for module:

| Scala | Rust | Notes |
|-------|------|-------|
| `ModuleProcessor` | `resolve` | Import graph, dotted-path resolution. **URL resolution replaces file paths** (D6). |
| `TheoryEnv` | `env` | Qualified-name environment, alias qualification, merge. |
| `InstInterpreter` + `InstInterpreterCases` | `interp` | ~38 KB of Scala. The core; the pushout semantics of D3 lives here. |
| `BasePresOps`, `LabelHelpers`, `AddEqRwHelpers` | `pres` | Presentation construction and manipulation. |
| `DesugarBinds` | `desugar` | Binder desugaring. |
| `Hypercube` | `hypercube` | Type-system generation. Port after the core lands. |
| `BNFCRenderer` | `render` | Emits the generated grammar. |

Pipeline: resolve -> env -> interpret -> check -> desugar -> hypercube ->
render. Retain the pass structure; the Scala `Pipeline`/`Pass` shape maps
directly and keeps each stage independently testable.

## 5. Phases

**Phase 0 — freeze the grammar.** Produce the normative grammar file from
`metta_venus.cf`: strike §3.3, re-notate terms per D2, add the per-group
`where` per D5, drop replacement profiles. No code. *Gate:* the corpus in §7
parses; the two `bad/` cases are rejected with named diagnostics.

**Phase 0.5 — unblock the parser.** The root `Cargo.toml` currently repoints
`rholang-parser`, `rholang-tree-sitter`, and `rholang-tree-sitter-proc-macro`
at a local worktree, `../rholang-rs-cost-accounting-transpiler/`. Only `master`
is pushed to `rholang-rs`, so the published branch is not buildable off the
machine that holds that worktree. Push the worktree as a branch, replace the
`[patch]` with a rev pin. *Gate:* clean clone of the branch builds. **Nothing
downstream can proceed in parallel until this is done.**

**Phase 1 — parser.** §4.1. *Gate:* the corpus round-trips; pin bumped in
f1r3node-rust; existing Rholang test suite unaffected.

**Phase 2 — elaborator.** §4.3, less Hypercube. *Gate:*
`InstInterpreterCasesSpec` ported and green (see §7 — this is the real
specification of the theory algebra, and D3 is only pinned down by it).

**Phase 3 — back ends and the payoff.** `BNFCRenderer`, the Hypercube pass,
presentation wired to the rewrite engine, and the elaborated presentation
connected to `oslf.rs`.

This last item is the reason for the whole exercise. A `where` guard today is
restricted to a pure boolean process. The papers claim its conditions are drawn
from a logic *generated from the language definition* — ground formulae and
spatial connectives supplied by the theory, alongside full boolean structure
and the context-labelled modality. Until a presentation reaches `oslf.rs`,
that claim is unsupported by anything in the tree. *Gate:* a `where` clause
mentioning a generated spatial connective compiles and runs, with an
inference-token budget bounding the check.

**Phase 4 — node integration.** Deploy path for elaborated artifacts, and
channels typed by the theory they carry.

**Phase 5 — documentation.** §6. Begins as soon as Phase 0 lands.

## 6. Documentation

Phase 5 does not wait on Phases 1–4. Freezing the grammar is what unblocks the
writing, and the documentation gap is the presenting complaint.

**6.1 `publications/MeTTaIL4WorkingDev`, part2.** Replace the `language!` block
as the sole exhibit with the pair: `language!` for the standalone case,
`UnivAlg -> ParMonoid -> QuoteDropCalc -> RhoCalc` for the reuse case. The
three-rung ladder (types+terms / +equations / +rewrites) survives intact and
gains a fourth rung — theory composition — which is the honest place to
introduce modules. Add a status table separating *specified* / *implemented* /
*planned*, so the document stops silently overclaiming; the existing status
table in part2 is the model. Remove the trailing-`where` form in favour of per
group.

**6.2 `publications/GSLT-intro/omnibus.tex` §9.** Same surface, same
correction. §9 is titled for a DDL whose data can wiggle and already works the
rubric four times; the four instances need re-notating, not rewriting.

**6.3 `f1r3node-rust/docs/mettail/`.** New. Reference documentation on the
model of `docs/rholang/02-syntax-reference.md`: the module surface, the theory
algebra with worked joins, the term judgement form, and the generated-logic
connection to `where`.

**6.4 `docs/plans/where-clauses-and-match-guards-2026-04-29.md`.** Add a
forward reference from §1.1 to this plan, since the guard sublanguage widens in
Phase 3.

## 7. Tests

**Corpus.** The eight `.module` files in `MeTTaIL/GSLT/src/test/module/`,
re-notated per D2 and stripped of Space per D4, become the parser corpus:
`Rholang`, `UnivAlg`, `List`, `ArrowCats`, `RenameRewrite`,
`ArithmeticOperations`, and the two negatives.

**Required diagnostics.** Two are taken from the existing negative cases; the
third is new, imposed by the ordered builder chain (§3.2):

- `bad/RepeatLabel.module` — duplicate label within one theory.
- `bad/ReplacementShadows.module` — a replacement whose target label collides
  with an existing label.
- **Forward reference in the chain** — an `Equations` or `Rewrites` block
  mentioning a label introduced by a later `Terms` block.

All three must be rejected with a named, located diagnostic, not a parse
failure.

**Conformance.** `InstInterpreterCasesSpec` (22 KB) ported verbatim as the
elaborator's acceptance suite. It is the only executable specification of the
theory algebra in existence; D3 in particular is asserted there and nowhere
else.

**New coverage.**

- Pushout sharing: `Rholang(nr, rc) { nr \/ rc }` yields exactly one `PPar`.
- URL import resolution, including the cycle and the unreachable-host cases.
- A `where` clause over a generated spatial connective (Phase 3).

## 8. Sequencing summary

```
Phase 0  (grammar freeze) ──┬─→ Phase 0.5 → Phase 1 → Phase 2 → Phase 3 → Phase 4
                            └─→ Phase 5 (documentation)
```

## 9. Open items

**9.1 URL imports and reproducibility.** D6 settles addressing but not
determinism: a bare URL resolved at two different times can yield two different
byte sequences. Under D7 this is contained — the client resolves at compile
time and the node receives elaborated artifacts — but a deploy is still only as
reproducible as the resolution that produced it. Recommendation: permit a
content hash alongside the URL (`import "<url>" ~ sha256:… as u`) and record
resolved hashes in a lock file. Needs a decision before Phase 2.

**9.2 The symmetric send.** D9 removes the symmetric receipts. `!$`
(`SendSymm`) sits in the `Proc` grammar rather than the `Receipt` section, so
D9 does not strike it automatically — but with no symmetric receipt left to
meet, it has no counterparty. Assumed struck; confirm. Nothing else in the
grammar references it, and `rholang-rs@master` implements neither side, so
this is a deletion from the reference grammar only.

**9.3 Collection sorts (G2).** `HashBag(Proc)` with `.*sep("|")` is the only
collection sort the withdrawn spec exercises. The form it replaces had `[Cat]`,
`Product { … }`, and the `separator`/`terminator` pragmas. Fix the vocabulary:
which of bag, set, list, product, and map are admitted, whether `Product`
survives, and whether each gets an inline rendering projection or only bags do.
Needed before Phase 0 closes, since it is grammar.

**9.4 BNFC pragmas.** `token`, `position token`, `coercions`, `entrypoints`,
`internal`, `layout` have no judgement-form counterpart. Retained as an escape
hatch, or dropped?

**9.5 The lollipop's footprint.** Per D8, `-o` leaves the delivered surface.
`examples/cost_accounting_demo.rho` and `docs/cost-accounting/demo.md` both
carry it and need amending; the demo's Cy scene either goes or is rewritten
onto two separately funded gates.
