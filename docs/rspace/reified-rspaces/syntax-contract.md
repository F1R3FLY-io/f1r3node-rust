# Reified RSpaces syntax contract

This contract states the surface syntax of Reified RSpaces and the exact [Par](glossary.md#par) that each form normalizes to.
A [frontend](glossary.md#frontend) that accepts the syntax must produce these Par values byte for byte.
The current frontend is the Tree-Sitter parser of `rholang-rs` with the node normalizers.
After the [F1r3lang](glossary.md#f1r3lang) merge of phase P12, the Rholang 1.4 frontend must produce the same bytes ([decision 9.2](decision-register.md#decision-9-2)).

The contract follows the [decision register](decision-register.md), which is normative.
Where this contract and the register differ, the register applies.

## Scope of this version

This version holds the surface sections of phase P3 and the run-time section of phase P4:

- the [terms](#terms) of the syntax
- the [grammar forms](#grammar-forms), with examples
- the [normalization mapping](#normalization-mapping) from each AST part to its builder and its [marker tuple](glossary.md#marker-tuple)
- the [errors](#errors) of the parser and of the normalizer
- the [keyword rule](#keyword-rule) of [decision 24](decision-register.md#decision-24) and the identifier rule of [decision 27](decision-register.md#decision-27)
- the [dual-frontend corpus](#dual-frontend-corpus) and [how the contract is checked](#how-the-contract-is-checked)
- the [meaning at run time](#meaning-at-run-time) of each construct in phase P4, and the constructs that still fail closed

Phase P12 adds the Rholang 1.4 productions.

## Terms

The table defines each term of this contract before its first use in the text below.
The [glossary](glossary.md) defines the terms that the table links.

| Term | Definition |
|---|---|
| Baseline grammar | The grammar of `rholang-rs` at commit `02cef80`. The node accepted exactly the programs of this grammar before the campaign. |
| v3 parser | The parser of the branch `feature/reified-rspaces-v3` at commit `6548aec`. It adds the Reified syntax to the baseline grammar. |
| Space | A [space](glossary.md#space): a tuple space that a program creates and uses as a value. |
| Space reference | A [space reference](glossary.md#space-reference): the value that selects a space, such as a handle that a [factory](glossary.md#factory) returns. A [space name](glossary.md#space-name) is another term: a channel of one space. |
| `use` block | The process `use s { P }`. The space reference `s` becomes the default space of the untagged channels of the process `P`. |
| Space annotation | The part `: s` of a binder `x : s` of `new`. It puts the new name `x` in the space `s`. |
| Plain binder | A binder of `new` without a URI, such as `x` or `x : s`. |
| URI binder | A binder of `new` with a system [URN](glossary.md#urn), such as ``out(`rho:io:stdout`)``. |
| Binding order | The order in which `new` binds its binders: the plain binders in source order, then the URI binders sorted by URI. |
| Theory reference | The value `free Th(args)`. It names the [theory](glossary.md#theory) `Th` with the arguments `args`. |
| Param | A hyperparameter of a send or of a query. A positional param is a process. A named param is `key=value`. |
| Param key | The identifier on the left of `=` in a named param. |
| Params section | The text after a `;` inside the parentheses of a send, a sync send, or a send-receive source: `x!(data; params)`. |
| Query | The part `~ q` after the source of a linear receive bind: `for (y <- ch ~ q) { P }`. |
| Payload | The processes of a query before its params, such as the vector of a similarity query. |
| Normalizer arm | The function of the node normalizer that turns one Reified AST part into Par. |
| Builder | A function of `models/src/rust/reified/builders.rs` that builds one marker tuple. It is the only code that builds marker tuples. |
| Validator | A check of `models/src/rust/reified/validation.rs`, such as the range of a param value. |
| Contextual keyword | A word that is a keyword only in some contexts. `use` and `free` are contextual keywords. In other contexts they stay ordinary identifiers. |
| Dual-frontend corpus | The file `test-vectors/reified-syntax-corpus-v1.json`. Each row pairs a source text with the bytes of its normalized Par. |

## Overview

Figure 1 shows how the node turns source text with Reified syntax into the bytes of a Par.
The [marker](glossary.md#marker) tuples carry each construct inside the existing fields of a Par ([decision 18](decision-register.md#decision-18)).
So the Protocol Buffers schema [[PB](#ref-pb)] does not change, and ordinary terms keep their bytes and hashes.

![Normalization of the Reified syntax as an activity. The source text first passes through preprocess_floats, the baseline rewrite of unsuffixed floats. The v3 parser follows: the external scanner reads use and free as keywords only when the next text can never continue a baseline program, and the AST builder walks each params section and each query once. A repeated key, a positional param after a named one, a key that is not an identifier, or a malformed query stops with a ParserError and no Par. The AST then holds UseBlock, NameDecl.space, FreeTheory, Hyperparam lists and QuerySpec. Five normalizer arms run in parallel branches: UseBlock with normalize_p_use_block and the builder use_block, New with annotations with normalize_p_new and new_space, FreeTheory with normalize_p_free_theory and efree, send params with normalize_p_send and send_params, and the receive query with normalize_p_input and query_spec. A rejection by a builder or a validator stops with a NormalizerError and no Par. Otherwise the marker tuples sit in their recognized positions, ParSortMatcher sorts the Par with no new sort score, and prost gives the bytes that the dual-frontend corpus pins. A legend gives the colors: Reified syntax, baseline code, and rejection.](diagrams/15-syntax-normalization.svg)

*Figure 1. Source: [diagrams/15-syntax-normalization.puml](diagrams/15-syntax-normalization.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

The figure has five steps:

1. The node first applies `preprocess_floats`, which is baseline code. Phase P11 retires it at the final parser pin.
2. The v3 parser reads the text. Its scanner decides whether `use` and `free` are keywords, by the [keyword rule](#keyword-rule).
3. The parser rejects a malformed params section or query by variant and span.
4. The normalizer sends each Reified AST part to its arm. Each arm normalizes the parts and calls one builder.
5. The builders and the validators check again each guarantee of the parser. The existing sorter then puts the tuples in [canonical form](glossary.md#canonical-form).

## Grammar forms

Each form below gives its production in the notation of `grammar.js`, the AST that the v3 parser builds, and examples.
Each `rholang` example is a row of the [dual-frontend corpus](#dual-frontend-corpus).
The productions show only the Reified alternatives of each rule.

### `use` blocks

```text
use_block := "use" name block
```

The keyword `use` comes from the external scanner, by the [keyword rule](#keyword-rule).
The space reference is any name: a variable, the wildcard, or a quoted process.
The block holds one process.
The parser builds `Proc::UseBlock { space, proc }`.

```rholang
use @"space" { @"x"!(1) }
```

```rholang
new s, t in { use s { use t { @"x"!(1) } } }
```

A quoted space reference with a compound process needs a block or parentheses, for example `use @{ x /\ y } { P }`.

### Space annotations on `new`

```text
name_decl := var [ ":" name | "(" uri ")" ]
```

A binder takes a space annotation or a URI, never both.
The parser stores the annotation in `NameDecl.space`.
A quoted name is a valid annotation.

```rholang
new s in { new x : s, y in { x!(1) | y!(2) } }
```

```rholang
new s in { new a, b : s, out(`rho:io:stdout`) in { b!(1) | out!(2) } }
```

### Theory references

```text
free_call := "free" var "(" [ proc { "," proc } ] ")"
```

The keyword `free` comes from the external scanner, by the [keyword rule](#keyword-rule).
A theory reference is an expression, so it is a value in every position.
The parser builds `Proc::FreeTheory { name, args }`.
The builtin theories `Nat`, `Int`, `String`, `Bool`, and `Any` parse here, because the theory name accepts the token `var`.

```rholang
@"factory"!(free Th(1, "two"))
```

### Params sections

```text
hyper_inputs   := "(" [ proc { "," proc } ] ";" [ param { "," param } ] ")"
param          := named_param | positional_param
named_param    := proc "=" proc
positional_param := proc
```

A parenthesized list with a `;` is a params section: the data come before the `;`, and the params come after it.
A list without a `;` keeps the shape of the baseline grammar, so the syntax tree of every ordinary send is unchanged ([decision 23](decision-register.md#decision-23)).
The same rule applies to `!`, `!!`, `!?`, the method sugar `x!m(…)`, and the `!?` and `!m` receive sources.

The grammar accepts any process as a key, and params in any order.
The AST builder then rejects a key that is not an identifier, a positional param after a named one, and a repeated key.
The parser stores the params as `Option<HyperparamList>`: `None` when the list has no `;`.

```rholang
@"x"!(1; 2, k=3)
```

```rholang
@"x"!(; k=1)
```

```rholang
for (y <- @"x"!?(1; k=1)) { Nil }
```

```rholang
new factory in { for (space <- factory!?("default", free Nat(); capacity=4)) { Nil } }
```

### Receive queries

```text
linear_bind := [ names ] "<-" source [ query_spec ]
query_spec  := "~" ( hyper_inputs | proc )
```

A query follows the parens-`;` rule of params sections:

| Query | Payload | Named params |
|---|---|---|
| `ch ~ e` | `[e]` | none |
| `ch ~ (a, b)` | `[(a, b)]`, one tuple | none |
| `ch ~ (a, b;)` | `[a, b]` | none |
| `ch ~ (a; k=v)` | `[a]` | `k` |
| `ch ~ (; k=v)` | empty | `k` |

The query `ch ~ (e;)` and the query `ch ~ e` give the same AST.
A query takes named params only, a query group is never empty, and only a linear bind with a simple source takes a query.
The parser builds `Bind::Linear { lhs, rhs, query_spec }`.

```rholang
for (@doc <- @"db" ~ ([0.5, 0.25]; metric="cosine", threshold=0.8, k=3)) { Nil }
```

```rholang
for (@task <- @"tasks" ~ (; priority=0)) { Nil }
```

```rholang
for (@a <- @"x" ~ 1 & @b <- @"y" where a == b) { Nil }
```

### Bare floats

The v3 grammar reads a float without a suffix, such as `0.85` or `1e3`, as a 64-bit float ([I-7](decision-register.md#i-7)).
A bare float needs a digit after its dot, so `5.toByteArray()` stays a method call on an integer.
Until the final parser pin, the node still applies `preprocess_floats` before it parses.
So the node normalizes every float form that the baseline accepts to its baseline bytes.

## Normalization mapping

The table maps each Reified AST part to its normalizer arm, the scope of its parts, its builder, and its marker tuple.
The arms are in `rholang/src/rust/interpreter/compiler/normalizer/processes/`.
The builders are in `models/src/rust/reified/builders.rs`.

| AST part | Normalizer arm | Scope of the parts | Builder | Marker tuple and its position |
|---|---|---|---|---|
| `Proc::UseBlock` | `normalize_p_use_block` in `p_use_block_normalizer.rs` | The space is a name and the body is a process, both in the current scope, the space first. | `use_block`, then `push_process_expr` | `(USE, s, P)` in `Par.exprs` |
| `NameDecl.space` on `Proc::New` | `normalize_p_new`, with `normalize_space_annotations` and `push_space_annotated_new` in `reified_syntax.rs` | Each annotation is a name in the scope outside the `new`, in binding order, before the body. | `new_space`, then `push_process_expr` | `(NEWSPACE, [a_1…a_k], Par{news: [New]})` in `Par.exprs` |
| `Proc::FreeTheory` | `normalize_p_free_theory` in `p_free_theory_normalizer.rs` | The arguments are processes in the current scope, in source order. | `efree`, then `push_process_expr` | the value `(FREE, "Th", [args…])` |
| `Proc::Send.params` | `normalize_p_send`, with `normalize_send_params` in `reified_syntax.rs` | The values follow the data in the current scope, in source order. | `send_params` | the last data element `(PARAMS, [p…], {k: v…})` |
| `Proc::SendSync.params` | `normalize_p_send_sync`, with `alloc_desugared_send` | The desugaring gives `new r in { x!(*r, d…; params) \| for (_ <- r) { cont } }`. | through the send arm | the last data element, after the reply name `*r` |
| `Source::SendReceive.params` | `normalize_p_input`, with `alloc_desugared_send` | The desugaring gives `new r in { x!(*r, d…; params) \| for (y <- r) { P } }`. | through the send arm | the last data element, after the reply name `*r` |
| `Bind::Linear.query_spec` | `normalize_p_input`, with `normalize_bind_source` in `reified_syntax.rs` | The channel, then the payload, then the params, in the scope outside the receive. | `query_spec` | the bind source `(QUERY, ch, [q…], {k: v…})` |

The marker of each tuple is the [unforgeable name](glossary.md#unforgeable-name) `GPrivate([0xE0, m])`, with `m` from `0xD0` (USE) to `0xD4` (FREE).
Each tuple has its [recognized position](glossary.md#recognized-position), and it is [inert](glossary.md#inert) data in every other position.

### Worked examples

| Source | Normalized Par, in tuple notation |
|---|---|
| `use @"space" { @"x"!(1) }` | `Par{exprs: [(USE, "space", @"x"!(1))]}` |
| `@"x"!(1; 2, k=3)` | `@"x"!(1, (PARAMS, [2], {"k": 3}))` |
| `@"x"!(1; b=2, a=1)` | `@"x"!(1, (PARAMS, [], {"a": 1, "b": 2}))` |
| `@"factory"!(free Th(1, "two"))` | `@"factory"!((FREE, "Th", [1, "two"]))` |
| `for (y <- @"x" ~ 5) { Nil }` | `for (y <- (QUERY, "x", [5], {})) { Nil }` |
| `new s in { new x : s in { x!(1) } }` | `new s in { (NEWSPACE, [[s]], new x in { x!(1) }) }` |

### Scopes and binding order

A `new` with $`n`$ binders in binding order $`b_0, \ldots, b_{n-1}`$ gives each binder a de Bruijn index in its body [[dB72](#ref-db72)].
In the body, the binder $`b_i`$ has the index $`n - 1 - i`$.
A name that has the index $`j`$ outside the `new` has the index $`j + n`$ in the body.
The annotations normalize outside the `new`, so an annotation that names an outer binder keeps the index $`j`$.

For example, in `new s in { new a, b : s, out(...) in { … } }` the binding order is `a`, `b`, `out`.
In the body, `out` has the index 0, `b` has 1, `a` has 2, and `s` has 3.
The annotation of `b` normalizes outside the inner `new`, where `s` has the index 0.
The NEWSPACE tuple holds one annotation entry for each plain binder, so the entry count is $`k = \mathit{bind\_count} - \lvert \mathit{uri} \rvert`$.
Each entry is `[]` for a binder without an annotation or `[s]` for a binder with one.

A query normalizes in the scope outside its receive.
So a query does not see the binders of its own receipt, but it sees the binders of earlier `;`-separated receipts.
For example, in `new y in { for (y <- @"x" ~ *y) { y!(1) } }` the query reads the outer `y`, and the body reads the bound `y`.

### Order of normalization

The normalizer gives each free variable a level in the order of its first occurrence.
The arms keep this order:

1. A send normalizes its channel, then its data, then its params.
2. A receive normalizes its patterns, then each bind source with its payload and its params, then its guard and its body.
3. A `new` normalizes its annotations, then its body.
4. A `use` block normalizes its space reference, then its body.

### Erased sections and canonical keys

The builders erase empty sections ([decision 21](decision-register.md#decision-21)):

- `x!(a;)` has the same bytes as `x!(a)`, because `send_params` returns no element for an empty list.
- `ch ~ (e;)` has the same bytes as `ch ~ e`.
- A `new` without an annotation has its baseline bytes, because the arm builds NEWSPACE only when some binder has an annotation.

Positional params keep their source order.
The builder sorts the named keys bytewise, so `@"x"!(1; b=2, a=1)` and `@"x"!(1; a=1, b=2)` have the same bytes.

### Ordinary programs keep their bytes

Each arm reads a Reified AST part only when the source has the construct.
Without the construct, each existing normalizer runs its baseline code path.
So every program that normalizes at the baseline normalizes to identical bytes with the v3 parser ([conservative extension](glossary.md#conservative-extension), consensus checklist item 22).

## Errors

### Parser errors

The v3 parser reports these errors by variant and span, with no message text ([decision 23](decision-register.md#decision-23), item UD-6).
The node reports a parse failure as `InterpreterError::ParserError` with the text `Parse failed: <variant> at <span>`, in debug form.

| Variant | Cause | Span |
|---|---|---|
| `DuplicateParamKey { key, first }` | A params section or a query repeats a key. | The repeated key. `first` is the span of its first occurrence. |
| `PositionalAfterNamed` | A positional param follows a named param in a send. | The positional param. |
| `NonIdentifierParamKey` | A key is not an identifier, for example `x!(a; 1 = 2)`. | The key. |
| `MalformedQuerySpec { reason }` | A query has a positional param, the empty group `~ (;)`, or a source `!?`, `?!`, or `!m`. | The param, the group, or the query. |

### Normalizer errors

The builders and the validators check again each guarantee of the parser.
A rejected construct gives `InterpreterError::NormalizerError` with the text `reified syntax: <message>`.
The message is the display text of a `ReifiedSyntaxError` variant.

| Variant | Message | Cause |
|---|---|---|
| `InvalidIdentifier` | `"<name>" is not a valid param key` or `"<name>" is not a valid theory name` | A key or a theory name that the parser cannot produce in its position. |
| `DuplicateKey` | `the key "<key>" occurs more than once` | A repeated key in a params section or a query. |
| `PositionalAfterNamed` | `a positional param follows a named param` | A positional param after a named param in a send. |
| `PositionalQueryParam` | `a query takes named params only` | A positional param in a query. |
| `KAndResults` | ``a query takes `k` or `results`, not both`` | A query with both `k` and `results`. |
| `EmptyQuery` | `a query needs a payload element or a named param` | A query with no payload and no named param. |
| `SpaceTypeOnUriBinding` | `a URI binder cannot take a space annotation` | A URI binder with a space annotation. |
| `OutOfRange` | `the value of "<key>" is not <expected>` | A literal value outside the range of its key, by the table below. |

The node adds one check of its own.
A query on a `!?`, `?!`, or `!m` source gives the message `reified syntax: receive query on a non-simple source`.
Such a query would apply to the fresh reply channel of the desugaring.

From source text, only `KAndResults` and `OutOfRange` can occur, because the parser rejects every other case first.
The other variants occur for an AST that a program builds by hand, and the tests build such an AST for each variant.
The arms never cause the variants `EmptyParams`, `NoAnnotation`, and `AnnotationCount`.
`send_params` erases an empty section instead of building an empty PARAMS tuple.
The `new` arm calls `new_space` only with one entry for each binder and with at least one annotation.

### Ranges of literal values

The arms apply `validate_param_value` to each named param whose value is a literal: `Nil`, a Boolean, a number, a string, or a URI.
The run-time layer checks the evaluated value of any other expression.

| Key | Range |
|---|---|
| `priority` | an `Int` at least 0 |
| `k` | an `Int` at least 1 |
| `results` | the string `"all"` |
| `threshold` | a float or a `BigRat` in the closed interval from 0 to 1 |
| `metric` | one of `cosine`, `dot`, `euclidean`, `manhattan`, `hamming`, and `jaccard` |

Every other key has no range in the syntax layer.

### Errors at run time

The reducer recognizes each marker tuple only in its recognized position ([decision 22](decision-register.md#decision-22)).
For example, a send whose last data element is a PARAMS tuple fails with `ParamsUnsupported` when its channel takes no params.
The section [Meaning at run time](#meaning-at-run-time) lists the errors of phase P4.
The [agent API](agent-api.md#10-aborts) gives the errors of each call, and the [design](design.md#n8-errors) lists every code.

## Meaning at run time

The reducer gives a meaning to a marker tuple only in its recognized position ([decision 22](decision-register.md#decision-22)).
In phase P4, three constructs have a meaning at run time: `use` blocks, space annotations, and params.
Every other construct fails closed with a deterministic code, until the phase that adds it ([decision 34](decision-register.md#decision-34), item A9).
A failure at run time aborts the deploy with `InterpreterError::ReduceError` and the text `reified:<Code>: <message>`.
The [design](design.md#n5-routing-and-scope) gives the algorithms, and the [specification](specification.md) gives the behavior of a space.

### `use` blocks at run time

The reducer evaluates the space reference `s` of `use s { P }` and classifies it by the five rules of a [space reference](glossary.md#space-reference).
When `s` names a native space, the reducer evaluates `P` with `s` as its [use context](glossary.md#use-context).
The use context changes the routes in `P` in three ways:

- A channel of the class `Forged`, such as `@"jobs"`, routes to its collection in the space `s` (rule R5).
- A plain binder of a `new` in `P` becomes a [space name](glossary.md#space-name) of `s`, through one allocation job for the space of `s` (F11).
- A body that the reducer stores for later evaluation, such as the body of a receive, keeps the use context, as a lexical closure does.

A space name and an ordinary unforgeable name keep their own routes in every use context.
The USE tuple takes the random state of the block, so it changes no random state of `P`.
A `use` block checks only the reference, so an operation in `P` on a space without a config record aborts with `DanglingHandle`.

A `new` with plain binders in `P` runs one allocation job for the space of `s`.
The job takes the cost authority of the process that evaluates the `new`.
For an Array or a Vector space, the job reads the config of the space and gives each binder an index name.
So a `new` inside `use` of a missing space aborts at the `new` with `DanglingHandle`, not at the first use of the name.
A `new` inside `use` of a full fixed Array space aborts with `OutOfNames`. A HashMap space keeps the names of a plain `new`.

```rholang
new hmq(`rho:space:hashmap:queue`) in { for (space <- hmq!?("default")) { use space { @"jobs"!(1) | for (@x <- @"jobs") { Nil } } } }
```

In this program, the send and the receive on `@"jobs"` meet in one collection of the new space, so the receive binds `x` to 1.

### Space annotations at run time

The reducer evaluates `new x_1 : s_1, …` in four steps:

1. It charges the binders, as the baseline `new` does.
2. It evaluates and classifies each annotation in the scope outside the `new`.
3. It draws the names as the baseline does, and it moves each binder with a space into its space.
4. It evaluates the body with the rest of the random state.

A binder with an annotation becomes a space name: the baseline id $`r`$ of the binder becomes $`\mathtt{0xE0} \mathbin{\Vert} \mathtt{N} \mathbin{\Vert} \mathit{sid} \mathbin{\Vert} r`$.
A binder without an annotation takes the space of the use context, and outside every `use` block it keeps its baseline name.
A URI binder keeps the value of its URN.

```rholang
new hmq(`rho:space:hashmap:queue`) in { for (space <- hmq!?("default")) { new box : space in { box!(1) | for (@x <- box) { Nil } } } }
```

In this program, `box` is a space name.
So the send and the receive on `box` meet in the new space in every context (rule R3).

### Params at run time

The send hook refreshes the derived flags of a trailing PARAMS tuple and decodes it strictly ([decision I-20](decision-register.md#i-20)).
A tuple that is not canonical aborts with `Malformed`.
The table gives the meaning of the params on each kind of channel.

| Channel of the send | Named params | Positional params |
|---|---|---|
| A factory | The constructor of the call holds them, and the gates of the factory check them against the schema of the family ([specification FA-4 to FA-7](specification.md#15-families)). | `PositionalParamsUnsupported` |
| A handle | `HANDLE_OP` checks them against the method. The method `produce` takes `persistent`, and another key gives the typed reply `UnknownParam`. | `PositionalParamsUnsupported` |
| A collection of a native space | The agent checks each key against the send schema of the discipline before the bump (`UnknownParam` with the closest key), and then each value (`InvalidParam`). The queue, the bag, the stack and the cell take no send param. | `PositionalParamsUnsupported`, at the hook |
| Every other channel | `ParamsUnsupported` | `ParamsUnsupported` |

```rholang
new hmq(`rho:space:hashmap:queue`) in { for (space <- hmq!?("default")) { for (_ <- space!?("produce", "jobs", [1]; persistent=true)) { Nil } } }
```

In this program, the handle call stores a persistent record with the payload `1` on the collection of `"jobs"`, and it replies `Nil`.
The [agent API](agent-api.md#5-handle-methods) gives every method and its params.

The factories of P5 take these constructor params ([specification FA-4 to FA-7](specification.md#15-families)):

| Construct | Rule | Failure |
|---|---|---|
| `F!?(…; size=n)` on an Array factory | a required `GInt` of at least 1 | `MissingParam`, `InvalidParam` |
| `F!?(…; cyclic=b)` on an Array factory | an optional `GBool`, `false` when absent | `InvalidParam` |
| `F!?(…; priorities=n)` on a priority queue factory | an optional `GInt` of at least 1, no bound when absent | `InvalidParam` |
| any other key | not in the schema of the family | `UnknownParam` with the nearest key within two edits |

### Constructs that fail closed

The table lists each construct that P4 rejects, with its code and the phase that adds its meaning.

| Construct | Example | Code | Phase |
|---|---|---|---|
| A query `~` on the default space, or on a discipline without query params | `for (y <- @"x" ~ 5) { Nil }` | `QuerySpecUnsupported` | P5 for the priority queue, P6 for VectorDB |
| A theory reference in a factory constructor | `hmq!?(free Nat())` | `TheoryUnavailable` | P7 |
| The qualifier `"temp"` or `"seq"` | `hmq!?("temp")` | `QualifierUnavailable` | P7 |
| A space reference that names a user agent | `use @"space" { @"x"!(1) }` | `AgentSpacesUnavailable` | P8 |
| A send or a receive on a name of a user agent | None in P4, because only a user agent makes such a name | `AgentSpacesUnavailable` | P8 |
| The handle method `consume` | `h!(ret, "consume", pairs, "<-", body)` | `AgentSpacesUnavailable`, or a typed reply | P8 |
| A join with a channel of a queue space | `for (@a <- x & @b <- y) { Nil }`, with `x` and `y` in one queue space | `OrderedJoinUnsupported` in P4 only | P5: wave W3 runs the join on the join engine ([specification 3.8](specification.md#38-joins)) |
| A join over two spaces, or over a space and the default space | `for (@a <- x & @b <- @"y") { Nil }`, with `x` in a space | `CrossSpaceJoin` | None: the plan keeps this abort |
| A factory of a family that is not registered yet, or its FIPS alias | ``new f(`rho:space:pathmap:bag`) in { Nil }`` | The baseline error `No value set for <urn>` | P5 wave W3 for PathMap, P6 for VectorDB, P7 for HashSet |

A query routes the inner channel of its QUERY tuple (`reified/reducer_hooks.rs:780-794`).
The inner channel takes the read rule of a receive source, with the same error text.
A query on the default space fails with `QuerySpecUnsupported` (`reified/reducer_hooks.rs:811-841`).
On a native space, the agent checks the query against the query params of the discipline. Hook H4 fails only on a malformed QUERY tuple (`reified/reducer_hooks.rs:740-751`).
A reference that names no space aborts with `EmptySpaceRef`, `HandleNotWritable`, or `NotASpaceRef` in every phase.

## Keyword rule

`use` and `free` are contextual keywords ([decision 24](decision-register.md#decision-24)).
In the baseline grammar they are ordinary identifiers, and many programs use them as names.
An external scanner of the v3 grammar emits a keyword only when the text that follows can never continue a baseline program.
In every other case, the internal lexer reads an ordinary identifier.
So every program that parses with the baseline grammar parses to the same tree with the v3 parser.

### When `use` is a keyword

The scanner emits the keyword `use` when all of these conditions hold:

1. The parser can start a `use` block at this point, and it is not in error recovery.
2. The text is `use`, and the next character cannot continue an identifier.
3. At least one whitespace character or comment follows.
4. A space reference follows: an identifier outside the USE table, the wildcard `_`, or `@` with a simple quoted process.
5. After optional whitespace and comments, the next character is `{`.

A simple quoted process is an identifier outside the QUOTE table, the wildcard, or a string, URI, or number literal.
A balanced group of `{ }`, `( )`, or `[ ]` is also a simple quoted process.

### When `free` is a keyword

The scanner emits the keyword `free` when all of these conditions hold:

1. Conditions 1 to 3 of `use` hold for the text `free`.
2. An identifier outside the FREE table follows.
3. After optional whitespace and comments, the next character is `(`.

### Exclusion tables

A word is in a table exactly when the form continues a baseline program in which `use` or `free` is a variable.
The script `scripts/derive_scanner_exclusions.js` of the parser derives the tables from the baseline `grammar.json`.

| Table | Form | Words |
|---|---|---|
| USE (11 words) | `use W {` | `and`, `bundle`, `bundle0`, `else`, `in`, `match`, `matches`, `not`, `or`, `select`, `where` |
| FREE (11 words) | `free W (` | `Set`, `and`, `else`, `for`, `if`, `in`, `match`, `matches`, `not`, `or`, `where` |
| QUOTE (5 words) | `use @W {` | `bundle`, `bundle0`, `match`, `not`, `select` |

For example, `use!(1)` and `new use in { use!(1) }` keep `use` as a name.
The text `free | Nil` keeps `free` as a name, and `free Int()` is a theory reference.

### Limits

- The baseline grammar reads the top-level text `use s { P }` as the three processes `use`, `s`, and `{ P }`. The node rejects every source with more than one top-level process, so no program that the node accepted changes.
- A compound quoted space reference needs a block or parentheses: `use @{ x /\ y } { P }`.
- A space reference or a theory name cannot be an excluded word of its form.

### Identifier rule

A param key or a theory name that a decoder accepts must be one that the frontend can produce ([decision 27](decision-register.md#decision-27)).
The two positions accept different words, so `models/src/rust/reified/validation.rs` holds two word lists:

- `REJECTED_PARAM_KEYS` (26 words): `Bool`, `ByteArray`, `Int`, `Nil`, `String`, `Uri`, `true`, `false`, and the 18 other baseline keywords except `catch`, `constructor`, `default`, `in`, `method`, and `private`.
- `REJECTED_THEORY_NAMES` (18 words): every baseline keyword except `Bool`, `ByteArray`, `Int`, `Nil`, `String`, `Uri`, `agent`, `bundle0`, `catch`, `constructor`, `default`, `method`, `private`, and `try`.

A word is a valid param key when it has the shape of the token `var` and is not in the first list.
A word is a valid theory name when it has the shape of the token `var` and is not in the second list.

## Dual-frontend corpus

The file `test-vectors/reified-syntax-corpus-v1.json` holds 40 rows.
Each row has two fields: `source`, the source text, and `expected_prost_hex`, the hexadecimal prost bytes of its normalized Par.
The file also records its name, the capture revisions of the node and of the parser, the capture command, and its rule.

The rows cover every construct of this contract:

- `use` blocks with a named, a quoted, and a nested space
- annotations with a name, with a quoted name, and beside a URI binder
- theory references with and without arguments, and the four builtin theories `Int`, `String`, `Bool`, and `Any` beside `Nat`
- params on `!`, `!!`, `!?`, `!m`, the `!?` source, and the `!m` source, positional and named, with and without data
- the erased section `@"x"!(1;)` and the ordinary send `@"x"!(1)`, which have the same bytes
- bare queries, groups with and without params, params-only groups, and payload lists
- queries in a join, with a guard, and in sequential receipts
- four programs of phase P4 that create a `hashmap:queue` space. They hold a `use` block, an annotation, a handle call with a named param, and a factory call with a theory

The corpus holds only rows that use `use` and `free` as keywords.
A row that uses them as identifiers is a regression test of the Tree-Sitter frontend, not a contract of every frontend.

To capture the corpus again after an approved change of the contract, do these steps from the repository root:

1. Set `RRS_CAPTURE_REIFIED_SYNTAX_OUT` to an absolute directory under `target/`.
2. Set `RRS_CAPTURE_REVISION`, `RRS_CAPTURE_PARSER_REVISION`, and `RRS_CAPTURE_COMMAND` to the provenance values.
3. Run `cargo test -p rholang --test reified_syntax_corpus reified_syntax_corpus_bytes_match`.
4. Review the difference between the captured file and the committed file.
5. Copy the captured file to `test-vectors/reified-syntax-corpus-v1.json`.

## How the contract is checked

These tests and gates check the contract:

| Check | What it shows |
|---|---|
| `rholang/tests/reified_syntax_normalize.rs` (27 tests) | Each arm builds the tuple of the builders, in the scope of the contract. The builders and the validators reject what the parser rejects. The property test `normalize_send_params_key_permutation_invariant` shows that the order of named params does not change the bytes. |
| `rholang/tests/reified_syntax_corpus.rs` (4 tests) | The normalizer gives the corpus bytes. The P2 decoders turn the corpus bytes into the constructs of a model that the test writes by hand. The corpus covers every construct. The property test `reified_syntax_corpus_ordinary_programs_have_no_markers` shows that ordinary programs give no marker. |
| `rholang/tests/reified_syntax_identifiers.rs` (1 test) | For every word-shaped string token of the parser grammar, the parser and the validators agree on param keys and on theory names ([decision 27](decision-register.md#decision-27)). |
| `rholang/tests/reified_source_normalization.rs` (2 tests) | Every repository source and every float form that the baseline accepts keep their bytes with the v3 parser. |
| `scripts/check-reified-rspaces-patch-gate.sh` | No staged or committed `Cargo.toml` holds the local parser `[patch]`. The option `--control` runs the same check on a negative-control fixture, which must fail. |

## References

<a id="ref-db72"></a>[dB72] N. G. de Bruijn, "Lambda calculus notation with nameless dummies, a tool for automatic formula manipulation, with application to the Church-Rosser theorem," *Indagationes Mathematicae (Proceedings)*, vol. 75, no. 5, pp. 381–392, 1972. [doi:10.1016/1385-7258(72)90034-0](https://doi.org/10.1016/1385-7258(72)90034-0)

<a id="ref-pb"></a>[PB] Google, "Protocol Buffers documentation." [protobuf.dev](https://protobuf.dev/)
