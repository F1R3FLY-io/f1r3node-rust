# TensorLogic syntax

This document specifies the surface syntax of TensorLogic (TL): its lexical rules, its grammar, its embedding in Rholang 1.4 and worked examples.
The terms of TL are defined in [concepts](concepts.md).
[Decision 64](../decision-register.md#decision-64) requires the approval of the user for this grammar before any code.

The surface has four layers, and each layer serves one purpose.

| Layer | Purpose | Constructs |
|---|---|---|
| Paper core | The index notation of the paper ([Dom25](#ref-dom25), section 3.1) | Equations, joins by juxtaposition, projections `=`, `max=`, `avg=`, element lists, slices, index arithmetic, `p'.`, `*t`, rules, facts, `?` |
| Functional layer | Pure, named computations | `let` functions, lambdas `x => e`, user operators |
| Rholang layer | Composition and privacy, with the meaning that Rholang gives them | `\|` (parallel composition), `new … in {…}`, `//` comments |
| Numpy-like sugar | Familiar positional forms that rewrite into the core | `@`, broadcasting, `.T`, `transpose`, `sum` and `max` with `axis=`, `einsum`, `softmax(axis=)`, slices |

Every sugar form rewrites into the core ([GSLT 3](gslt.md#3-the-sugar-and-its-rewrites)), so two programs that compute the same thing have one normal form.

## Contents

1. [Lexical structure](#1-lexical-structure)
2. [The grammar](#2-the-grammar)
3. [The embedding in Rholang 1.4](#3-the-embedding-in-rholang-14)
4. [Worked examples](#4-worked-examples)
5. [Differences from numpy](#5-differences-from-numpy)

The [references](#references) follow the last section.

## 1. Lexical structure

### 1.1 Constraints from the FLT host

A TL term is the raw text of a foreign-language term (FLT) inside Rholang 1.4.
F1r3lang opens an FLT body in three forms: ``tl:Cat`…` `` with backticks, a triple-backtick fence, and `tl:Cat{…}` with braces.
The lexer of F1r3lang defines these forms in lines 648 to 668 of `languages/src/rholang.rs` (mettail-rust at `c68bebf5`).
Four constraints follow from that lexer.

1. No space separates the category from the opening brace: `tl:Score{…}`, never `tl:Score {…}`.
2. In the brace form, braces must balance, because the body lexer counts their depth.
3. A dollar sign starts a hole `${…}` and is not allowed elsewhere, because a guest chunk excludes it.
4. Host comments do not apply inside the body.

So TL uses no dollar sign and no backtick, and it uses braces only in balanced blocks.
A string literal with an unbalanced brace needs the fence form.

### 1.2 Whitespace and comments

Whitespace (space, tab, carriage return and line feed) separates tokens and has no other meaning.
A comment is `//` to the end of the line, or `/* … */` with no nesting ([decision 64](../decision-register.md#decision-64)).
If the definition language of F1r3lang cannot declare comments for a guest language, comments go in the Rholang around the FLT.
Missing facility MF6 of the [design rationale](design-rationale.md#4-missing-f1r3lang-facilities) asks for that declaration.

### 1.3 Identifiers

TL identifiers follow a profile of Unicode Standard Annex 31 (UAX #31) ([UAX31](#ref-uax31)).

```ebnf
Ident = Start , { Continue } , { Prime } ;
Prime = "'" | "′" | "″" | "‴" | "⁗" ;
```

- `Start` is a character with the property XID_Start, or the underscore.
- `Continue` is a character with the property XID_Continue, except the two middle dots U+00B7 and U+0387. A middle dot would make `W·X` one identifier, so an identifier with a middle dot fails with `ConfusableIdentifier` ([GSLT 2.1](gslt.md#21-the-tokens)).
- A prime is the ASCII single quote. The Unicode primes U+2032, U+2033, U+2034 and U+2057 normalize to one, two, three and four ASCII quotes ([decision 64](../decision-register.md#decision-64)).
- Identifiers compare after normalization form C (NFC) ([UAX15](#ref-uax15)).

**Security.** The rules of Unicode Technical Standard 39 (UTS #39) protect against look-alike names ([UTS39](#ref-uts39)).

- Every character must have the Identifier_Status Allowed of the General Security Profile.
- Every identifier must use a single script, where the scripts Common and Inherited mix with any script (open question K6).
- An identifier whose skeleton equals the skeleton of a built-in name, or of another identifier of the same term with another spelling, fails with `ConfusableIdentifier`. For example, a Cyrillic `р` in `relu` fails.

**Reserved spellings.** The keywords of [section 2](#2-the-grammar) are reserved.
An underscore followed by digits is reserved for the canonical names of the normal form.
In score and check terms, the reserved names of [section 3.3](#33-how-data-enters) are reserved too.

**Unicode version.** The checker pins one version of the Unicode data, and the checker identity records it (open question K14).

**User operators.** A user may define an infix operator with a name of one non-ASCII character of the general category Sm (math symbol) or So (other symbol).
An operator whose skeleton equals an ASCII operator or an identifier fails with `NonAsciiOperator`, with the ASCII spelling as the fix.
Examples: U+2212 `−` has the skeleton `-`, and `×` has the skeleton `x` (open question K5).
Built-in operators, symbols, functions and methods are pure ASCII ([decision 64](../decision-register.md#decision-64)).

### 1.4 Numbers

Numerals follow the Numeric Types FIPS, with exact values as [decision 54](../decision-register.md#decision-54) requires ([NTF25](#ref-ntf25)).

```ebnf
Numeral = Digits , [ "." , Digits ] , [ ( "e" | "E" ) , [ "+" | "-" ] , Digits ] , [ Suffix ] ;
Digits  = Digit , { [ "_" ] , Digit } ;
Suffix  = "n" | "r" | "f32" | "f64" | "p" , Digits ;
```

| Form | Meaning |
|---|---|
| No suffix | The exact decimal value: `0.1` is $`1/10`$ (open question K4) |
| `n`, `r` | The same exact value, as an integer or a rational |
| `p` followed by digits | The same exact value, as a fixed-point decimal |
| `f64`, `f32` | The exact dyadic value of $`\mathrm{RN}_{53}`$ or $`\mathrm{RN}_{24}`$ of the decimal, in the exponent range of IEEE binary64 or binary32. An overflow fails with `LiteralRange`. |

- The modular suffixes of the FIPS (`i8`, `u32` and the others) fail with `LiteralKind`, because TL arithmetic is not modular.
- A numeral has no sign. The minus sign is always an operator (rule R1 of [section 2.4](#24-ambiguity-rules)).
- The lexer reads a numeral and the identifier characters that follow it as one token ([GSLT 2.1](gslt.md#21-the-tokens)). So `2x` fails with `LiteralKind`, because `x` is not a suffix. Write `2 x` or `2*x`.

### 1.5 Strings

A string is `"…"` with the escapes of Rholang.
It denotes a symbolic atom equal to the Rholang `GString` of the same text.
The single quote never starts a string, because it is the prime.

### 1.6 Operators and punctuation

All built-in operators and punctuation are ASCII.

```text
|  <-  ,  ?  :  =  +=  max=  min=  avg=  maj=  =>  or  and  not
==  !=  <  <=  >  >=  +  -  *  /  %  @  **  [  ]  (  )  {  }  .  '
```

The projection tokens `max=`, `min=`, `avg=` and `maj=` have no inner space.
The star also marks a virtual index at the start of an index atom (rule R9).

## 2. The grammar

### 2.1 The grammar in EBNF

The grammar uses the EBNF of ISO/IEC 14977.
The entry points are the four FLT categories of [section 3.1](#31-categories).

```ebnf
(* ---------- Entry points: the FLT categories ---------- *)
Program  = Stmt , { "|" , Stmt } ;
Score    = Expr
         | "let" , Def , "in" , "{" , Score , "}" ;
Check    = Expr
         | "let" , Def , "in" , "{" , Check , "}"
         | Ident , ":" , Shape , "in" , "{" , Check , "}" ;   (* the Ident must be v *)

(* ---------- Statements: the Rholang layer ---------- *)
Stmt     = Equation | Rule | Fact | Query | Decl
         | "new" , Ident , { "," , Ident } , "in" , "{" , Program , "}"
         | "let" , Def , "in" , "{" , Program , "}"
         | "{" , Program , "}" ;
Equation = Lhs , ProjOp , ( Expr | ElemList ) ;
ProjOp   = "=" | "+=" | "max=" | "min=" | "avg=" | "maj=" ;
Lhs      = Ident
         | Ident , "[" , [ LhsIx , { "," , LhsIx } ] , "]"     (* numeric *)
         | Ident , "(" , [ LhsIx , { "," , LhsIx } ] , ")" ;   (* Boolean *)
LhsIx    = Ix , [ "." ] ;                                     (* "." marks a normalized index *)
Rule     = Lhs , "<-" , Expr , { "," , Expr } ;
Fact     = Lhs ;                                              (* static: a ground Boolean Lhs *)
Query    = Lhs , "?" ;
Decl     = Ident , ":" , Shape ;
Shape    = [ "bool" ] , "[" , [ Dim , { "," , Dim } ] , "]" ;
Dim      = Ix | "sym" ;

(* ---------- The functional layer ---------- *)
Def      = Ident , "(" , [ Params ] , ")" , "=" , Body        (* function *)
         | Ident , "=" , Lambda                                (* function as a lambda *)
         | Ident , OpSym , Ident , "=" , Body                  (* user infix operator *)
         | Ident , "=" , Expr ;                                (* named constant *)
Params   = Ident , { "," , Ident } ;
Lambda   = Ident , "=>" , Body
         | "(" , [ Params ] , ")" , "=>" , Body ;
Body     = Expr | "{" , Expr , "}" ;

(* ---------- Expressions ---------- *)
Expr     = Conj , { "or" , Conj } ;
Conj     = Neg , { "and" , Neg } ;
Neg      = "not" , Neg | Cmp ;
Cmp      = UOp , [ CmpOp , UOp ] ;                             (* non-associative *)
CmpOp    = "==" | "!=" | "<" | "<=" | ">" | ">=" ;
UOp      = Add , { OpSym , Add } ;                             (* static: one OpSym per chain *)
Add      = Term , { ( "+" | "-" ) , Term } ;
Term     = Unary , { ( "*" | "/" | "%" | "@" ) , Unary | Power } ;   (* a bare Power is juxtaposition *)
Unary    = ( "-" | "+" ) , Unary | Power ;
Power    = Post , [ "**" , Unary ] ;                           (* right-associative *)
Post     = Atom , { "[" , [ Sub , { "," , Sub } ] , "]" | "." , "T" } ;
Atom     = Ident | Numeral | String | "(" , Expr , ")"
         | Ident , "(" , [ Arg , { "," , Arg } ] , ")"           (* user call or Boolean access *)
         | FnKw , "(" , [ Arg , { "," , Arg } ] , ")"            (* built-in call *)
         | AggKw , "[" , Ident , { "," , Ident } , "]" , "(" , Expr , ")"   (* explicit projection *)
         | NormKw , "[" , Ident , "]" , "(" , Expr , [ "," , Expr ] , ")" ; (* explicit normalization *)
AggKw    = "sum" | "max" | "min" | "avg" | "maj" ;
NormKw   = "softmax" | "lnorm" ;
FnKw     = "step" | "relu" | "abs" | "sign" | "min" | "max" | "clamp" | "hardsig"
         | "exp" | "log" | "sqrt" | "rsqrt" | "tanh" | "sin" | "cos" | "sig" | "sigmoid"
         | "softmax" | "lnorm" | "concat" | "sum" | "transpose" | "einsum" ;
Arg      = Expr | ElemList | Ident , "=" , ( Expr | ElemList ) ;     (* keyword argument: axis=… *)
Sub      = Ix | [ Ix ] , ":" , [ Ix ] , [ ":" , [ Ix ] ] ;          (* an index or a numpy slice *)
ElemList = "[" , [ Elem , { "," , Elem } ] , "]" ;
Elem     = Expr | ElemList ;

(* ---------- Index expressions ---------- *)
Ix       = IxTerm , { ( "+" | "-" ) , IxTerm } ;
IxTerm   = IxUnary , { ( "*" | "/" | "%" ) , IxUnary } ;   (* "/" divides by a positive constant and floors *)
IxUnary  = "-" , IxUnary | IxAtom ;
IxAtom   = [ "*" ] , Ident | Numeral | String | "(" , Ix , ")" ;    (* a leading "*" marks a virtual index *)
```

**Keywords.** The words `let`, `in`, `new`, `not`, `and` and `or` are reserved, and so is every word of `FnKw`, `AggKw` and `NormKw`.
The words `T` after a dot, and `bool` and `sym` in a shape, are contextual.
If guest languages of F1r3lang cannot declare contextual keywords, `T` is reserved too (missing facility MF7).

Figures TL3 and TL4 draw the grammar as railroad diagrams.
A rounded box is a nonterminal of the grammar, and a plain box is a token.

![Railroad diagram of the programs and statements of TensorLogic. A Program is one or more statements separated by the parallel bar. A statement is an equation, a rule, a fact, a query, a declaration, a new block, a let block, or a program in braces. An equation is a left side, a projection operator and an expression or an element list. A projection operator is equals, plus-equals, max-equals, min-equals, avg-equals or maj-equals. A left side is a name, a name with indices in brackets, or a name with indices in parentheses for a Boolean tensor, and a left index may carry a dot that marks a normalized index. A rule is a left side, the rule arrow and literals separated by commas. A query is a left side with a question mark. A declaration is a name, a colon and a shape.](../diagrams/37-tl-grammar-statements.svg)

*Figure TL3. Source: [diagrams/37-tl-grammar-statements.puml](../diagrams/37-tl-grammar-statements.puml), rendered by [diagrams/render.sh](../diagrams/render.sh).*

![Railroad diagram of the expressions and index expressions of TensorLogic. An expression is a chain of or, and, not and comparisons. Below the comparisons come user operators, then sums with plus and minus, then terms, where a term joins factors by juxtaposition or combines them with times, divide, modulo or the matrix product. Below the terms come the unary signs and the power operator, which is right-associative. A postfix form indexes an atom with subscripts or transposes it with dot T. An atom is a name, a numeral, a string, a parenthesized expression, a call of a user function or a Boolean access, a built-in call, an explicit projection with its bound indices, or an explicit normalization along one index. An index expression is an affine sum of index terms, which multiply, divide or take the remainder by constants, and an index atom may start with a star that marks a virtual index.](../diagrams/38-tl-grammar-expressions.svg)

*Figure TL4. Source: [diagrams/38-tl-grammar-expressions.puml](../diagrams/38-tl-grammar-expressions.puml), rendered by [diagrams/render.sh](../diagrams/render.sh).*

### 2.2 Precedence and associativity

The table lists the levels from the loosest binding to the tightest binding.

| Level | Construct | Associativity | Nonterminal |
|---|---|---|---|
| 0 | `\|` between statements | Commutative and associative (a bag) | Program |
| 1 | `=`, `+=`, `max=` and the other projections, `<-` with `,`, `:`, postfix `?` | None | Stmt |
| 2 | `=>` | Right, in `let` only | Lambda |
| 3 | `or` | Left | Expr |
| 4 | `and` | Left | Conj |
| 5 | `not` | Prefix | Neg |
| 6 | `==`, `!=`, `<`, `<=`, `>`, `>=` | None | Cmp |
| 7 | User Unicode operators | Left. Two different operators never mix (`OperatorMix`). | UOp |
| 8 | `+`, `-` | Left | Add |
| 9 | Juxtaposition, `*`, `/`, `%`, `@` | Left | Term |
| 10 | Unary `-` and `+` | Prefix | Unary |
| 11 | `**` | Right. `-a**2` is `-(a**2)`, and `a ** -1` is allowed. | Power |
| 12 | Indexing `[…]`, `.T`, calls, `op[j](…)` | Left | Post, Atom |

User operators sit between the comparisons and the addition, as the infix functions of Kotlin do.

### 2.3 Statement separators

Only `|` separates statements.
A line break never separates statements, because juxtaposition would join a term across the line break.

### 2.4 Ambiguity rules

The grammar has sixteen points where a reader could see two readings.
Each rule below picks one reading.

| Rule | Situation | Reading |
|---|---|---|
| R1 | Juxtaposition and binary minus | A juxtaposed factor is a `Power`, which cannot start with a sign. So `a -b` is `a - b`. Write `a (-b)` or `a * -b` to multiply by a negated factor. |
| R2 | Juxtaposition and a call | `Ident (` is always a call, with or without a space, as in Python. Write `w * (…)` to join a name with a group in parentheses. |
| R3 | A call and a Boolean access | Built-in names are keywords. `f(…)` calls a user function when a `let` binds `f`, and it is a Boolean access otherwise ("parentheses mean Boolean", [Dom25](#ref-dom25), p. 6). |
| R4 | `[` after a name, `]`, `)` or `T` | Indexing. An element list appears only as a whole right side, as an argument or as an element, never juxtaposed. |
| R5 | `'` | Only the prime suffix. Strings use `"`. |
| R6 | Parentheses around an expression | They always group. A Boolean reading comes only from `Ident(…)` and from Boolean tensor kinds. |
| R7 | The `=` family | `=` at statement level is the sum projection, in a call it marks a keyword argument, and in `let` it defines. `==` compares. `max(` is a call, and `max[` is a projection. |
| R8 | `:` | A slice inside `[…]`, a declaration at statement level, and a shape annotation in a check term |
| R9 | `*` | At the start of an index atom it marks a virtual index. Between two operands it multiplies. |
| R10 | `.` | After a left-side index it marks a normalized index, on a plain variable only. `.T` transposes. A decimal point has digits on both sides. |
| R11 | `?` | Only a statement suffix |
| R12 | `\|` | Never an expression operator. Logic uses `or`. |
| R13 | `<-` | One token. Write `a < -b` for a comparison with a negated value. |
| R14 | A bare name in an expression | First a `let` constant or a size variable, then an index variable of the equation (its natural-number value), then a tensor. A name that is two of these fails with `AmbiguousName`. |
| R15 | Score and check bodies | Expressions with optional `let` or shape prefixes, never statements. So a bare `R(c)` is never both a fact and an expression. |
| R16 | Implicit summation | A semantic rule, not a syntactic one ([static semantics 3](static-semantics.md#3-the-scope-rule)) |

Rule R2 needs a parser preference, because the grammar alone allows two readings.
If the parser of F1r3lang cannot state that preference, the token `Ident(` without a space carries the call (missing facility MF4).

## 3. The embedding in Rholang 1.4

### 3.1 Categories

The TL language exposes five FLT categories.

| Category | Body | Use |
|---|---|---|
| `tl:Program` | `Program` | TL-1 and TL-2b programs through `rho:lang:vector`, and its methods `normalize`, `check` and `demand` |
| `tl:Score` | `Score` | TL-0: the value of the `score` query param |
| `tl:Check` | `Check` | TL admission theories ([integration 6](integration.md#6-admission-theories)) |
| `tl:Stmt`, `tl:Expr` | One statement or one expression | FLT patterns, holes and composition |

A Datalog rule is a `tl:Stmt`, so TL has no separate rule category.

### 3.2 Binding the handle

A program obtains the handle `tl` of the installed language with the idiom of the F1r3lang fixtures.
The idiom is in lines 14 to 21 and 497 to 514 of `rholang-runtime/tests/fixtures/regex_gslt_application.rho` (mettail-rust at `c68bebf5`).

```rholang14
new install(`rho:mettail:install`), loaded, handlePipe, notify,
    getTl(`rho:lib:1.0.0:<publisher-hex>:tensorlogic:1.0.0`) in {
  for (@spec <- getTl!?(*notify)) { install!(spec, *loaded) } |
  for (@installed <- loaded) {
    handlePipe!(installed.get("ok").get("exports_by_name").get("TensorLogic"))
  } |
  for (tl <- handlePipe) {
    // the TL terms of this block are written tl:Score{…}, tl:Program{…} and so on
    Nil
  }
}
```

The URI binder for `rho:lib:…` follows the Versioned Registry FIPS ([VRF25](#ref-vrf25)).
The fixture itself calls the registry with an explicit lookup, so the node must still confirm the URI-binder form.
The Lookahead FIPS offers another form, `let tl = free TensorLogic() in {…}` ([LAF26](#ref-laf26)). The choice is open question K13, which the user decides together with the owner of F1r3lang.

### 3.3 How data enters

A construction hole cannot be the data path of a score, because the agent, not the program, supplies each record.
So data enters a TL term through reserved names that the agent binds, and through a named-tensor map param ([decision 64](../decision-register.md#decision-64)).
Finding F1 of the [README](README.md#4-the-findings-of-the-design-pass) notes that holes now take any lexical binding, and open question K1 asks whether holes become a second path later.

| Category | Reserved names, bound by the agent | Other data |
|---|---|---|
| `tl:Score` | `x`: the index of the record, symbolic and record-local<br>`Emb[x, d]`: the embedding of the record, with `d` in $`[0, D)`$<br>`q[d]`: the query vector, which is the payload of the query<br>`D`: the dimensions of the space | `tensors = {"Name": value, …}` |
| `tl:Check` | `v`: the checked value as a tensor<br>`D`, when the term declares `v : [D]` | `tensors` in the args of the theory |
| `tl:Program` | None | The `tensors` argument of `eval` |

**The tensor value ABI.** One encoding of tensors serves the `tensors` map, the inputs and the outputs.

| Form | Rholang value | TL tensor |
|---|---|---|
| V0. Number | A number of any kind of [decision 54](../decision-register.md#decision-54) | A tensor of rank 0 |
| V1. Dense | A rectangular nested `EList` | Positional dimensions. An empty list gives a dimension of size 0. |
| V2. Relation | An `ESet` or an `EList` of `ETuple`s of atoms, or bare atoms for arity 1 | A `Bool` tensor over symbolic dimensions |
| V3. Keyed | An `EMap` from an atom or a tuple of atoms to V0 or V1 | Mixed symbolic and positional dimensions, for example `{"Alice": [0.1, 0.9]}` |

- A tuple and a list never mean the same thing: dense elements are numbers or lists, never tuples.
- A named input that the term does not use is not decoded.
- A name in `tensors` must be a valid TL identifier, and it must not shadow a reserved name (`ReservedName`).

Figure TL19 shows the four forms with an example of each.

![Diagram of the tensor value ABI. Four Rholang values map to four TL tensors. A number, such as the integer 42, maps to a tensor of rank 0. A rectangular nested list, such as two rows of two numbers, maps to a dense tensor with positional dimensions of sizes 2 and 2. A set of tuples of atoms, such as the pairs Alice Bob and Bob Charlie, maps to a Boolean tensor over two symbolic dimensions whose listed pairs are 1. A map from atoms to lists, such as Alice to the list 0.1 and 0.9, maps to a tensor with one symbolic and one positional dimension, whose values are the exact binary values of the two floats. Notes say that a tuple is never a dense element and that a name must not shadow a reserved name.](../diagrams/39-tl-value-abi.svg)

*Figure TL19. Source: [diagrams/39-tl-value-abi.puml](../diagrams/39-tl-value-abi.puml), rendered by [diagrams/render.sh](../diagrams/render.sh).*

### 3.4 A TL score in a VectorDB query

At P12 the query vocabulary gains the key `score`, as [decision 65](../decision-register.md#decision-65) foresees.
The example ranks records by a bilinear form with a weight matrix `W` and returns the best two.

```rholang14
for (@[best, second] <- @"docs" ~ ([0.85, 0.1, 0.1, 0.2];
       score = tl:Score{ Emb[x, d] W[d, e] q[e] },
       tensors = {"W": [[1, 0, 0, 0], [0, 2, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]]},
       k = 2)) {
  stdout!([best, second])
}
```

The `QUERY` tuple carries the reflected term as a value (lines 546 to 560 of `encoding.rs`), so the node grammar does not change ([GSLT 7](gslt.md#7-no-change-to-the-node-grammar)).
With the records of the P6 study, the scores are $`33/40`$ for cat, $`103/400`$ for ml and $`117/200`$ for hiking.
So the query returns cat and hiking.

## 4. Worked examples

Each example is a complete TL program or score, and each one parses under the grammar of [section 2](#2-the-grammar) with one reading.
The normal form (NF) of an example uses readable names here, and the normalizer renames them to `_0`, `_1` and so on.

### 4.1 Perceptron (paper p. 6)

```tensorlogic
W = [0.2, 1.9, -0.7, 3] | X = [0, 1, 1, 0] | Y = step(W[i] X[i]) | Y?
```

The index `i` is not on the left side, so the projection sums over it inside `step`.
The NF is `Y = step(sum[i](W[i] X[i]))`, and the value is $`H(19/10 - 7/10) = 1`$.
The sugar form is `Y = step(W @ X)`.

### 4.2 No summation (paper p. 6)

```tensorlogic
W = [0.2, 1.9, -0.7, 3] | X = [0, 1, 1, 0] | Y[i] = step(W[i] X[i]) | Y?
```

Here `i` is on the left side, so nothing is summed, and `Y` is `[0, 1, 0, 0]`.
The sugar form is `Y = step(W * X)`.

### 4.3 One layer of a network

```tensorlogic
H[i] = relu(W[i, j] X[j] + B[i]) | H?
```

The NF is `H[i] = relu(sum[j](W[i, j] X[j]) + B[i])`, and the sugar form is `H = relu(W @ X + B)`.

### 4.4 Multilayer perceptron (paper p. 7)

```tensorlogic
W : [L, J, J] | X[0, j] = Input[j] | X[l + 1, j] = sig(W[l, j, k] X[l, k]) | Out[j] = X[L, j] | Out?
```

The recursion is guarded on `l`: each read of `X` is at a smaller layer than the write.
The declaration of `W` gives `l` a hard domain, so the recursion ends ([static semantics 7](static-semantics.md#7-recursion-classes-and-stratification)).
The virtual form `X[*l + 1, j] = sig(W[l, j, k] X[*l, k])` keeps only the newest layer.

### 4.5 Recurrent network (paper p. 7)

```tensorlogic
U : [J, T] | X[i, 0] = Init[i] | X[i, *t + 1] = sig(W[i, j] X[j, *t] + V[i, j] U[j, t]) | X?
```

The declaration of `U` gives `t` the domain $`[0, T)`$, and the virtual index keeps one state at a time.

### 4.6 Convolution and pooling (paper p. 9)

```tensorlogic
Image : [W0, H0, C] | Filter : [K, K, C] | Features : [W0 - K + 1, H0 - K + 1] |
Features[x, y] = relu(Filter[dx, dy, ch] Image[x + dx, y + dy, ch]) |
Pooled[x / 2, y / 2] max= Features[x, y] | Pooled?
```

Index arithmetic reads the image at an offset, and the left side `Pooled[x / 2, y / 2]` groups two-by-two blocks under `max=`.

### 4.7 Graph neural network (paper Table 1)

```tensorlogic
Emb[n, 0, d] = X[n, d] |
Z[n, l, d'] = relu(WP[l, d', d] Emb[n, l, d]) |
Agg[n, l, d] = Neig(n, n') Z[n', l, d] |
Emb[n, l + 1, d] = relu(WAgg[l, d, e] Agg[n, l, e] + WSelf[l, d, e] Emb[n, l, e]) |
Y[n] = sig(WOut[d] Emb[n, L, d]) | Y?
```

The paper writes scalar aggregation weights, and this example writes them as one matrix per layer.

### 4.8 Attention head (paper p. 10)

```tensorlogic
Query[p, dk] = WQ[dk, d] X[p, d] | Key[p, dk] = WK[dk, d] X[p, d] | Val[p, dv] = WV[dv, d] X[p, d] |
Comp[p, p'.] = softmax(Query[p, dk] Key[p', dk] / sqrt(Dk)) |
Attn[p, dv] = Comp[p, p'] Val[p', dv] | Attn?
```

The mark `p'.` normalizes along `p'`, so the NF of `Comp` is `Comp[p, p'] = softmax[p'](sum[dk](Query[p, dk] Key[p', dk] / sqrt(Dk)))`.
The sugar form of the whole head is below.

```tensorlogic
Q = X @ WQ.T | K = X @ WK.T | V = X @ WV.T | A = softmax(Q @ K.T / sqrt(Dk), axis=-1) @ V | A?
```

### 4.9 Transformer block (paper Table 2)

```tensorlogic
Query[h, p, dk] = WQ[h, dk, d] S[p, d] |
Key[h, p, dk] = WK[h, dk, d] S[p, d] |
Val[h, p, dv] = WV[h, dv, d] S[p, d] |
Comp[h, p, p'.] = softmax(Query[h, p, dk] Key[h, p', dk] / sqrt(Dk)) |
Attn[h, p, dv] = Comp[h, p, p'] Val[h, p', dv] |
Merge[p, dm] = concat(Attn[h, p, dv]) |
Out[p, d.] = lnorm(WS[d, dm] Merge[p, dm] + S[p, d]) |
PosEnc[p, d] = (d % 2 == 0) sin(p / L ** (d / De)) + (d % 2 == 1) cos(p / L ** ((d - 1) / De)) |
Out?
```

- `concat` elaborates to the left index `h * Dv + dv`.
- The paper writes `Even(d)`, and this example writes a comparison, which elaborates to a step function.

### 4.10 Kernel machine (paper section 4.3)

```tensorlogic
K[q, i] = exp(-(Xq[q, j] - Xs[i, j]) ** 2 / Var) | Pred[q] = sig(A[i] Lab[i] K[q, i] + B) | Pred?
```

The index `j` is summed outside the square, as the Gaussian kernel needs ([static semantics 3](static-semantics.md#3-the-scope-rule)).
In the polynomial kernel `K[q, i] = (Xq[q, j] Xs[i, j]) ** N`, the index `j` is summed inside the power.

### 4.11 Datalog (paper pp. 2-5)

```tensorlogic
Parent("Alice", "Bob") | Parent("Bob", "Charlie") |
Ancestor(x, y) <- Parent(x, y) |
Ancestor(x, z) <- Ancestor(x, y), Parent(y, z) |
Ancestor("Alice", x)?
```

The result is the set of `"Bob"` and `"Charlie"`.
The tensor form of the recursive rule is `Ancestor(x, z) = Ancestor(x, y) Parent(y, z)`.
Stratified negation is allowed: `Orphan(x) <- Person(x), not HasParent(x)`.

### 4.12 Embedding reasoning (paper section 5)

```tensorlogic
S[d] = V(x) Emb[x, d] |
Member[a] = sig(S[d] Emb[a, d] - 1/2, 0) |
EmbR[i, j] = R(x, y) Emb[x, i] Emb[y, j] |
Dab[a, b] = EmbR[i, j] Emb[a, i] Emb[b, j] |
Sim[x, x'] = Emb[x, d] Emb[x', d] |
Member? | Dab? | Sim?
```

The second argument of `sig` is the temperature, and the temperature 0 makes the sigmoid a step (open question K7).

### 4.13 VectorDB scores (TL-0)

| Score | Term | Query params |
|---|---|---|
| Dot product | `tl:Score{ Emb[x, d] q[d] }` | `k = 2` |
| Gaussian similarity | `tl:Score{ exp(-(Emb[x, d] - q[d]) ** 2 / 2) }` | `threshold = 0.5` |
| Negated squared distance | `tl:Score{ -(Emb[x, d] - q[d]) ** 2 }` | `threshold = -0.09` |
| Reasoning at a temperature | `tl:Score{ sig(EmbR[i, j] q[i] Emb[x, j] - 1/2, 1/10) }` | `tensors = {"EmbR": embR}`, `threshold = 0.5` |

With the records cat, ml and hiking of the P6 study and the query `[0.85, 0.1, 0.1, 0.2]`, the dot scores are $`41/50`$, $`13/80`$ and $`29/50`$.
So `k = 2` returns cat and hiking.

### 4.14 Bayesian network (paper section 4.4)

```tensorlogic
PB[b] = CPTB[b, a] PA[a] | PE = PB[b] Evidence[b] | Cond[b] = PB[b] Evidence[b] / PE | Cond?
```

The first equation marginalizes over `a`, and the last equation conditions on the evidence.

### 4.15 Majority and sigmoid (FIPS line 261)

```tensorlogic
V = [[1, 0, 1, 1], [1, 1, 0, 0], [1, 1, 1, 0]] | B[i] maj= V[n, i] | B?
```

The column sums are 3, 2, 2 and 1 over three rows, so `B` is `[1, 1, 1, 0]`.
An elementwise sigmoid is `Y = sig(X)` in positional form and `Y[i] = sig(X[i], 1/2)` with a temperature.

### 4.16 A TL-1 call

```rholang14
new vec(`rho:lang:vector`), stdout(`rho:io:stdout`) in {
  for (@r <- vec!?("eval",
                   tl:Program{ Y[i] = relu(W[i, j] X[j]) | Y? },
                   {"W": [[1, -1], [2, 0]], "X": [3, 4]},
                   {"precision": 64})) {
    match r {
      [true, out]          => stdout!(out)
      [false, code, detail] => stdout!([code, detail])
    }
  }
}
```

The reply is `[true, {"Y": [0, 6]}]`: the first row gives $`\mathrm{relu}(3 - 4) = 0`$ and the second row gives $`\mathrm{relu}(6) = 6`$.

### 4.17 Rules set at a time (TL-2b)

```rholang14
new vec(`rho:lang:vector`), stdout(`rho:io:stdout`) in {
  @"kb"!(Set(("Alice", "Bob"), ("Bob", "Charlie"))) |
  for (@kb <<- @"kb") {
    for (@r <- vec!?("eval",
                     tl:Program{ Ancestor(x, y) <- Parent(x, y) |
                                 Ancestor(x, z) <- Ancestor(x, y), Parent(y, z) |
                                 Ancestor("Alice", x)? },
                     {"Parent": kb},
                     {})) {
      stdout!(r)
    }
  }
}
```

The relation enters as one value (form V2), so the program sees one snapshot of it.

## 5. Differences from numpy

The sugar looks like numpy, and [decision 63](../decision-register.md#decision-63) lists where it differs.

| Topic | Numpy | TL |
|---|---|---|
| Arithmetic | IEEE floating point with dtype promotion | Exact rationals. `/` is exact, and no value is rounded except by a declared transcendental function. |
| Transcendental functions | Rounded by the platform library | Correctly rounded at a declared precision ([dynamic semantics 3](dynamic-semantics.md#3-correctly-rounded-functions)) |
| Mutation | In-place updates | No mutation. A program is a set of equations. |
| Shapes | Can depend on data (Boolean or fancy indexing) | Static. No Boolean indexing, no fancy indexing, no iterative solvers. |
| `einsum` | Supports the ellipsis | The ellipsis fails with `EinsumEllipsis`. |
| Broadcasting | The numpy rules | The same rules, checked before evaluation, with diagnostics that name the shapes and the axes ([static semantics 4](static-semantics.md#4-broadcasting-and-the-positional-sugar)) |

## References

<a id="ref-dom25"></a>[Dom25] P. Domingos, "Tensor Logic: The Language of AI," arXiv:2510.12269v3, 2025. [doi:10.48550/arXiv.2510.12269](https://doi.org/10.48550/arXiv.2510.12269)

<a id="ref-laf26"></a>[LAF26] "Lookahead," F1r3fly Improvement Proposal, 2026-01-08. FIPS repository, approved/2026-01-08-Lookahead.

<a id="ref-ntf25"></a>[NTF25] "Numeric Types," F1r3fly Improvement Proposal, 2025-11-13. FIPS repository, approved/2025-11-13-Numeric-Types.

<a id="ref-uax15"></a>[UAX15] The Unicode Consortium, "Unicode Normalization Forms," Unicode Standard Annex 15. [unicode.org/reports/tr15](https://unicode.org/reports/tr15/)

<a id="ref-uax31"></a>[UAX31] The Unicode Consortium, "Unicode Identifiers and Syntax," Unicode Standard Annex 31. [unicode.org/reports/tr31](https://unicode.org/reports/tr31/)

<a id="ref-uts39"></a>[UTS39] The Unicode Consortium, "Unicode Security Mechanisms," Unicode Technical Standard 39. [unicode.org/reports/tr39](https://unicode.org/reports/tr39/)

<a id="ref-vrf25"></a>[VRF25] "Versioned Registry," F1r3fly Improvement Proposal, 2025-09-16. FIPS repository, approved/2025-09-16-Versioned-Registry.
