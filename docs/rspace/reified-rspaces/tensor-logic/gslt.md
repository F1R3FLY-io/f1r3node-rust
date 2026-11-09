# The TensorLogic GSLT

This document defines TensorLogic (TL) as a GSLT of F1r3lang: its categories, its constructors, its equations and its rewrites.
It also defines the normal form (NF) and the reflection of TL terms to Rholang values.
Last, it defines the content identity of the language, the capability that evaluates TL, and TL admission theories.
The terms are defined in [concepts](concepts.md), and the grammar is in [syntax](syntax.md).

[Decision 62](../decision-register.md#decision-62) assigns the work to two parts.
The GSLT gives the syntax and the typing of TL terms.
A native, metered evaluator gives their values, because an installed F1r3lang theory has no rational arithmetic (finding F3 of the [README](README.md#4-the-findings-of-the-design-pass)).

## Contents

1. [The formalism](#1-the-formalism)
2. [The TensorLogic module](#2-the-tensorlogic-module)
3. [The sugar and its rewrites](#3-the-sugar-and-its-rewrites)
4. [The core](#4-the-core)
5. [Normalization](#5-normalization)
6. [The reflection of TL terms](#6-the-reflection-of-tl-terms)
7. [No change to the node grammar](#7-no-change-to-the-node-grammar)
8. [Content identity, installation and versioning](#8-content-identity-installation-and-versioning)
9. [The capability rho:lang:vector](#9-the-capability-rholangvector)
10. [Admission theories](#10-admission-theories)
11. [The OSLF view](#11-the-oslf-view)

The [references](#references) follow the last section.

## 1. The formalism

A GSLT is written as a `Module` that holds a `Theory` in the definition language (DDL) of F1r3lang.
The DDL is part of the Rholang 1.4 grammar of F1r3lang, in `languages/src/rholang.rs` (mettail-rust at `c68bebf5`).
The design uses only the DDL constructs below, and the table gives the lines of that file that define each construct.

| Construct | Meaning | Lines of `languages/src/rholang.rs` |
|---|---|---|
| `Module Name { … }` and `Theory Name() { … }` | A module and its theory | 2760 to 2765 |
| `Types { … }` with `noadmit` and carriers such as `= String` | The categories. `noadmit` means that a category holds no object-level variables. A carrier gives a category host values. | 2724 to 2725, 2767 to 2773 |
| `Terms { … }` with `Label . x:Sort, … \|- syntax : Category;` | Constructors in judgment form | 2730 to 2731, 2784 to 2789 |
| `token Name ::= regex;` | A token of the guest lexer, for a category with a carrier | 2790 to 2791, and the regex lexer modes, 612 and 671 to 684 |
| `HashBag(X)`, `List(X)` and `xs.*sep("…")` | Collection sorts and their projections | 2807 to 2810, 2812 to 2813 |
| `Equations { lhs == rhs; }` | Equations between terms | 2732 to 2733, 2822 to 2825 |
| `Rewrites { Name : lhs ~> rhs; }` and `if S ~> T then …` | Rewrite rules, and congruence premises | 2734 to 2735, 2827 to 2828, 2839 to 2842 |
| `Options { Semantics { Limits { … } } }` | Resource limits of the theory | 2736 to 2737, 2774 to 2775 |

A complete theory in this formalism is the regex GSLT of the F1r3lang fixtures (`rholang-runtime/tests/fixtures/regex_gslt_application.rho`, lines 14 to 495).
That fixture shows four facts that the TL module relies on.

- A category with a carrier, such as `noadmit Scalar = String;`, gets its lexical form from a `token` declaration of the same name.
- `noadmit` categories still accept FLT holes.
- A rewrite applies at a position below the root only through a congruence rule, such as `NullableSurfaceStep : if S ~> T then (NullableSurface S) ~> (NullableSurface T);` (line 202).
- The limit keys are `max_term_nodes`, `max_proof_nodes`, `max_frontier` and `max_grade_bits` (lines 485 to 490).

The PraTTaIL regex compiler is the only validator of a token regex (`mettail-elab/src/wire.rs`, lines 592 to 595).
It accepts Unicode property classes such as `\p{XID_Start}`, code-point escapes such as `\u{2032}`, and complemented code-point classes.
Lines 10 to 29 and 1260 to 1270 of `prattail/src/automata/regex.rs` document and implement these classes.
It has no intersection or subtraction of classes, so the module writes the operator token as one complemented class.

## 2. The TensorLogic module

The module below gives every category and every constructor of TL.
The constructors follow the grammar of [syntax 2](syntax.md#2-the-grammar) one to one.
The label of a constructor is its name in the reflected term ([section 6](#6-the-reflection-of-tl-terms)).
Constructors marked "derived" are sugar, and [section 3](#3-the-sugar-and-its-rewrites) rewrites them into the core.

```rholang14
Module TensorLogicGSLT {
  Theory TensorLogic() {
    Types {
      noadmit Program; noadmit Stmt; noadmit Score; noadmit Check;
      noadmit Lhs; noadmit LhsIx; noadmit ProjOp; noadmit Shape; noadmit Dim;
      noadmit Def; noadmit Body; noadmit Lam; noadmit Conjs;
      noadmit Expr; noadmit Conj; noadmit Neg; noadmit Cmp; noadmit CmpOp; noadmit UOp;
      noadmit Add; noadmit Term; noadmit Unary; noadmit Power; noadmit Post; noadmit Atom;
      noadmit FnKw; noadmit AggKw; noadmit NormKw; noadmit Arg; noadmit Sub;
      noadmit ElemList; noadmit Elem; noadmit Ix; noadmit IxTerm; noadmit IxUnary; noadmit IxAtom;
      noadmit Ident = String;   noadmit OpSym = String;
      noadmit Numeral = String; noadmit Text = String;
    }
    Terms {
      // Tokens. The native checker reads the text of each token again (section 2.1).
      token Ident ::= [\p{XID_Start}_][\p{XID_Continue}]*['\u{2032}-\u{2034}\u{2057}]*;
      token Numeral ::= [0-9](_?[0-9])*(\.[0-9](_?[0-9])*)?([eE](\+|-)?[0-9](_?[0-9])*)?[\p{XID_Continue}]*;
      token Text ::= "([^"\\]|\\.)*";
      token OpSym ::= [^\u{0}-\u{7F}\p{L}\p{M}\p{N}\p{P}\p{Z}\p{C}\p{Sk}\p{Sc}\p{XID_Continue}];

      // Programs: the Rholang layer. | is a bag, so it is commutative and associative.
      PComp . ss:HashBag(Stmt) |- ss.*sep("|") : Program;
      SGroup . p:Program |- "{" p "}" : Stmt;
      SNew . ns:List(Ident), p:Program |- "new" ns.*sep(",") "in" "{" p "}" : Stmt;
      SLet . d:Def, p:Program |- "let" d "in" "{" p "}" : Stmt;
      SEq . l:Lhs, o:ProjOp, e:Expr |- l o e : Stmt;
      SDef . l:Lhs, o:ProjOp, v:ElemList |- l o v : Stmt;
      SRule . h:Lhs, b:Conjs |- h "<-" b : Stmt;                         // derived
      SFact . h:Lhs |- h : Stmt;
      SQuery . h:Lhs |- h "?" : Stmt;
      SDecl . t:Ident, s:Shape |- t ":" s : Stmt;
      ConjOne . e:Expr |- e : Conjs;                                     // derived
      ConjMore . e:Expr, r:Conjs |- e "," r : Conjs;                     // derived
      OpSum . |- "=" : ProjOp;
      OpAcc . |- "+=" : ProjOp;                                          // derived
      OpMax . |- "max=" : ProjOp;
      OpMin . |- "min=" : ProjOp;
      OpAvg . |- "avg=" : ProjOp;
      OpMaj . |- "maj=" : ProjOp;
      LBare . t:Ident |- t : Lhs;
      LNum . t:Ident, ix:List(LhsIx) |- t "[" ix.*sep(",") "]" : Lhs;
      LBool . t:Ident, ix:List(LhsIx) |- t "(" ix.*sep(",") ")" : Lhs;
      XPlain . i:Ix |- i : LhsIx;
      XNorm . i:Ix |- i "." : LhsIx;                                     // derived
      ShNum . ds:List(Dim) |- "[" ds.*sep(",") "]" : Shape;
      ShBool . ds:List(Dim) |- "bool" "[" ds.*sep(",") "]" : Shape;
      DIx . i:Ix |- i : Dim;
      DSym . |- "sym" : Dim;

      // The entry categories of scores and checks
      ScExpr . e:Expr |- e : Score;
      ScLet . d:Def, s:Score |- "let" d "in" "{" s "}" : Score;
      CkExpr . e:Expr |- e : Check;
      CkLet . d:Def, c:Check |- "let" d "in" "{" c "}" : Check;
      CkDecl . v:Ident, s:Shape, c:Check |- v ":" s "in" "{" c "}" : Check;

      // The functional layer: every constructor is derived, by inlining
      DFun . f:Ident, ps:List(Ident), b:Body |- f "(" ps.*sep(",") ")" "=" b : Def;
      DLam . f:Ident, l:Lam |- f "=" l : Def;
      DOp . a:Ident, o:OpSym, b:Ident, e:Body |- a o b "=" e : Def;
      DConst . c:Ident, e:Expr |- c "=" e : Def;
      BExpr . e:Expr |- e : Body;
      BBrace . e:Expr |- "{" e "}" : Body;
      LamOne . p:Ident, b:Body |- p "=>" b : Lam;
      LamMany . ps:List(Ident), b:Body |- "(" ps.*sep(",") ")" "=>" b : Lam;

      // Expressions, one category for each precedence level
      EOr . a:Expr, b:Conj |- a "or" b : Expr;                           // derived
      EOfConj . c:Conj |- c : Expr;
      EAnd . a:Conj, b:Neg |- a "and" b : Conj;                          // derived
      COfNeg . n:Neg |- n : Conj;
      ENot . n:Neg |- "not" n : Neg;                                     // derived
      NOfCmp . c:Cmp |- c : Neg;
      ECmp . a:UOp, o:CmpOp, b:UOp |- a o b : Cmp;                       // derived
      CmOfU . u:UOp |- u : Cmp;
      CEq . |- "==" : CmpOp;
      CNe . |- "!=" : CmpOp;
      CLt . |- "<" : CmpOp;
      CLe . |- "<=" : CmpOp;
      CGt . |- ">" : CmpOp;
      CGe . |- ">=" : CmpOp;
      EUser . a:UOp, o:OpSym, b:Add |- a o b : UOp;                      // derived
      UOfAdd . a:Add |- a : UOp;
      EAdd . a:Add, b:Term |- a "+" b : Add;
      ESub . a:Add, b:Term |- a "-" b : Add;
      AOfTerm . t:Term |- t : Add;
      EJuxt . a:Term, b:Power |- a b : Term;                             // the join
      EMul . a:Term, b:Unary |- a "*" b : Term;
      EDiv . a:Term, b:Unary |- a "/" b : Term;
      EMod . a:Term, b:Unary |- a "%" b : Term;
      EMatMul . a:Term, b:Unary |- a "@" b : Term;                       // derived
      TOfU . u:Unary |- u : Term;
      ENeg . u:Unary |- "-" u : Unary;
      EPos . u:Unary |- "+" u : Unary;
      UOfP . p:Power |- p : Unary;
      EPow . b:Post, e:Unary |- b "**" e : Power;
      POfPost . p:Post |- p : Power;
      EIdx . p:Post, ss:List(Sub) |- p "[" ss.*sep(",") "]" : Post;
      ET . p:Post |- p "." "T" : Post;                                   // derived
      PofA . a:Atom |- a : Post;
      AName . n:Ident |- n : Atom;
      ANum . n:Numeral |- n : Atom;
      AStr . s:Text |- s : Atom;
      AParen . e:Expr |- "(" e ")" : Atom;
      AApp . f:Ident, as:List(Arg) |- f "(" as.*sep(",") ")" : Atom;
      ACall . f:FnKw, as:List(Arg) |- f "(" as.*sep(",") ")" : Atom;
      AProj . o:AggKw, js:List(Ident), e:Expr |- o "[" js.*sep(",") "]" "(" e ")" : Atom;
      ANorm . o:NormKw, k:Ident, e:Expr |- o "[" k "]" "(" e ")" : Atom;
      ANormArg . o:NormKw, k:Ident, e:Expr, a:Expr |- o "[" k "]" "(" e "," a ")" : Atom;
      KSum . |- "sum" : AggKw;
      KMax . |- "max" : AggKw;
      KMin . |- "min" : AggKw;
      KAvg . |- "avg" : AggKw;
      KMaj . |- "maj" : AggKw;
      KSoftmax . |- "softmax" : NormKw;
      KLnorm . |- "lnorm" : NormKw;
      FStep . |- "step" : FnKw;
      FRelu . |- "relu" : FnKw;
      FAbs . |- "abs" : FnKw;
      FSign . |- "sign" : FnKw;
      FMin . |- "min" : FnKw;
      FMax . |- "max" : FnKw;
      FClamp . |- "clamp" : FnKw;
      FHardsig . |- "hardsig" : FnKw;
      FExp . |- "exp" : FnKw;
      FLog . |- "log" : FnKw;
      FSqrt . |- "sqrt" : FnKw;
      FRsqrt . |- "rsqrt" : FnKw;
      FTanh . |- "tanh" : FnKw;
      FSin . |- "sin" : FnKw;
      FCos . |- "cos" : FnKw;
      FSig . |- "sig" : FnKw;
      FSigmoid . |- "sigmoid" : FnKw;                                    // derived
      FSoftmax . |- "softmax" : FnKw;                                    // derived
      FLnorm . |- "lnorm" : FnKw;                                        // derived
      FConcat . |- "concat" : FnKw;                                      // derived
      FSum . |- "sum" : FnKw;                                            // derived
      FTranspose . |- "transpose" : FnKw;                                // derived
      FEinsum . |- "einsum" : FnKw;                                      // derived
      ArgE . e:Expr |- e : Arg;
      ArgL . l:ElemList |- l : Arg;
      ArgKw . k:Ident, e:Expr |- k "=" e : Arg;
      ArgKwL . k:Ident, l:ElemList |- k "=" l : Arg;

      // Subscripts: an index, or a numpy slice start:end:step with optional parts
      SubIx . i:Ix |- i : Sub;
      SubAll . |- ":" : Sub;
      SubFrom . a:Ix |- a ":" : Sub;
      SubTo . b:Ix |- ":" b : Sub;
      SubRange . a:Ix, b:Ix |- a ":" b : Sub;
      SubAllS . |- ":" ":" : Sub;
      SubFromS . a:Ix |- a ":" ":" : Sub;
      SubToS . b:Ix |- ":" b ":" : Sub;
      SubRangeS . a:Ix, b:Ix |- a ":" b ":" : Sub;
      SubAllStep . c:Ix |- ":" ":" c : Sub;
      SubFromStep . a:Ix, c:Ix |- a ":" ":" c : Sub;
      SubToStep . b:Ix, c:Ix |- ":" b ":" c : Sub;
      SubStep . a:Ix, b:Ix, c:Ix |- a ":" b ":" c : Sub;

      LList . es:List(Elem) |- "[" es.*sep(",") "]" : ElemList;
      ElE . e:Expr |- e : Elem;
      ElL . l:ElemList |- l : Elem;

      // Index expressions
      IxAdd . a:Ix, b:IxTerm |- a "+" b : Ix;
      IxSub . a:Ix, b:IxTerm |- a "-" b : Ix;
      IxOfT . t:IxTerm |- t : Ix;
      IxMul . a:IxTerm, b:IxUnary |- a "*" b : IxTerm;
      IxDiv . a:IxTerm, b:IxUnary |- a "/" b : IxTerm;
      IxMod . a:IxTerm, b:IxUnary |- a "%" b : IxTerm;
      IxOfU . u:IxUnary |- u : IxTerm;
      IxNeg . u:IxUnary |- "-" u : IxUnary;
      IxOfA . a:IxAtom |- a : IxUnary;
      IxVar . v:Ident |- v : IxAtom;
      IxVirt . v:Ident |- "*" v : IxAtom;
      IxNum . n:Numeral |- n : IxAtom;
      IxStr . s:Text |- s : IxAtom;
      IxPar . i:Ix |- "(" i ")" : IxAtom;
    }
    Equations {
      // E4 and E5: every left-nested chain of joins, and every left-nested chain of sums,
      // is invariant under a permutation of its operands.
      (EJuxt (EJuxt a b) c) == (EJuxt (EJuxt a c) b);
      (EJuxt (TOfU (UOfP x)) y) == (EJuxt (TOfU (UOfP y)) x);
      (EAdd (EAdd a b) c) == (EAdd (EAdd a c) b);
      (EAdd (AOfTerm x) y) == (EAdd (AOfTerm y) x);
    }
    Rewrites {
      // S1: += is the sum projection.
      AccIsSum : (OpAcc) ~> (OpSum);
      // S2: a rule is an equation whose body joins its literals, each in parentheses.
      RuleOne : (SRule h (ConjOne e)) ~> (SEq h (OpSum) e);
      RuleFold : (ConjMore e (ConjOne f)) ~> (ConjOne (EOfConj (COfNeg (NOfCmp (CmOfU (UOfAdd (AOfTerm (EJuxt (TOfU (UOfP (POfPost (PofA (AParen e))))) (POfPost (PofA (AParen f)))))))))));
      // Congruence: every position where S1 and S2 can apply
      ProgramStep : if S ~> T then (PComp {S, ...rest}) ~> (PComp {T, ...rest});
      GroupStep : if S ~> T then (SGroup S) ~> (SGroup T);
      NewStep : if S ~> T then (SNew ns S) ~> (SNew ns T);
      LetStep : if S ~> T then (SLet d S) ~> (SLet d T);
      EquationOpStep : if S ~> T then (SEq l S e) ~> (SEq l T e);
      DefinitionOpStep : if S ~> T then (SDef l S v) ~> (SDef l T v);
      RuleBodyStep : if S ~> T then (SRule h S) ~> (SRule h T);
    }
    Options { Semantics { Limits { max_term_nodes = 4096; max_proof_nodes = 4096; max_frontier = 64; max_grade_bits = 64; } } }
  }
  theory TensorLogic()
}
```

### 2.1 The tokens

The four tokens give the lexer a superset of the lexical rules of [syntax 1](syntax.md#1-lexical-structure).
The RESOLVE phase of the native normalizer reads the text of each token again and enforces the exact rules ([section 5.1](#51-the-pipeline)).

| Token | What the lexer accepts | What RESOLVE adds |
|---|---|---|
| `Ident` | A start character, continue characters and primes, as in [syntax 1.3](syntax.md#13-identifiers) | NFC, the primes in ASCII form, the rules of UTS #39 and the reserved spellings. A middle dot fails with `ConfusableIdentifier`, because it looks like a product. |
| `Numeral` | The digits, the fraction and the exponent, and then every identifier character that follows | The suffix must be one of [syntax 1.4](syntax.md#14-numbers). So `2x` and `2e` fail with `LiteralKind`, and `2 x` is a join. |
| `Text` | A string with the escapes of Rholang | The escapes and the UTF-8 text |
| `OpSym` | One character outside ASCII that is a math symbol (Sm) or another symbol (So), and is not an identifier character | The skeleton check of `NonAsciiOperator` |

The complement class of `OpSym` removes every general category except Sm and So, and it removes the identifier characters.
So `OpSym` and `Ident` never accept the same character, and the ASCII operators stay tokens of their own.

### 2.2 The equations

The installed equations are a sound subset of the equational theory $`E_{TL}`$ of [section 5.2](#52-why-the-normal-form-is-unique).
The four equations make the operands of a left-nested join chain, or of a left-nested sum chain, commute.
The program composition `|` is a bag (`HashBag`), so the bag sort gives its commutativity and its associativity, and no equation is needed for it.
The native normalizer applies the whole of $`E_{TL}`$, which also regroups chains across parentheses (equation E3).

### 2.3 The installed rewrites

The installed rewrites remove the sugar S1 and S2, which can only occur in statements.
Seven congruence rules reach every position of a statement.
They descend into the bag of a program and into the bodies of `{…}`, `new` and `let`.
They also reach the projection operator of an equation and the body of a rule.
`RuleFold` folds the literals of a rule from the right end, one at a time.
The other sugar forms act inside expressions, so [section 3](#3-the-sugar-and-its-rewrites) gives them to the native normalizer.

**Points that P12 validates against the DDL parser.** The code at `c68bebf5` leaves four details open.

1. Whether the four token declarations compile, and whether the keyword literals take precedence over the `Ident` token, as they do in the host grammar.
2. Whether the undelimited projection `ss.*sep("|")` of a `HashBag` parses. The host bag `PPar` has braces (line 790). If it does not parse, `PComp` becomes a binary constructor with the two equations of commutativity and associativity.
3. The leaf of a token with a `String` carrier: `Label("text")` (`rholang-codegen/src/rho_net_lower.rs`, lines 1773 to 1776) or `^ident("text")` (line 4199 of that file).
4. Whether every example of [syntax 4](syntax.md#4-worked-examples) parses with exactly one reading.

The first P12 step parses this module with the DDL parser of F1r3lang and records each answer ([design rationale 5](design-rationale.md#5-implementation-order)).
No answer changes the meaning of TL, because the native normalizer decides every TL value.

Figure TL12 shows the categories and how the derived constructors map into the core.

![Class diagram of the TensorLogic GSLT. The categories Program, Stmt, Score and Check are the entry points. Program is a bag of statements. Stmt has the core constructors SEq, SDef, SFact, SQuery, SDecl, SNew, SLet and SGroup, and the derived constructor SRule, which rewrites to SEq. The expression categories form a chain from Expr through Conj, Neg, Cmp, UOp, Add, Term, Unary and Power to Post and Atom. Derived constructors such as EOr, EAnd, ENot, ECmp, EUser, EMatMul, ET and the positional calls rewrite into core constructors such as EJuxt, EAdd, EMul, EDiv, EPow, EIdx, ACall with step, AProj and ANorm. Index expressions form the chain Ix, IxTerm, IxUnary and IxAtom.](../diagrams/47-tl-gslt-categories.svg)

*Figure TL12. Source: [diagrams/47-tl-gslt-categories.puml](../diagrams/47-tl-gslt-categories.puml), rendered by [diagrams/render.sh](../diagrams/render.sh).*

## 3. The sugar and its rewrites

Every sugar form rewrites into the core ([decision 63](../decision-register.md#decision-63)).
A rewrite of class I is installed in the GSLT, and the native normalizer also runs it.
A rewrite of class N runs only in the native normalizer.

| Id | Sugar | Rewrite into the core | Class |
|---|---|---|---|
| S1 | `+=` | `=` | I |
| S2 | `H <- L1, …, Ln` | `H = (L1) … (Ln)` with a Boolean left side | I |
| S3 | `a > b` and `a < b` | `step(a - b)` and `step(b - a)` | N |
| S4 | `a >= b` and `a <= b` | `1 - step(b - a)` and `1 - step(a - b)` | N |
| S5 | `a == b` and `a != b` | `(1 - step(a - b)) (1 - step(b - a))` and `step(a - b) + step(b - a)` | N |
| S6 | `not a`, `a and b`, `a or b` | `1 - a`, `a b`, `step(a + b)`, on Boolean operands | N |
| S7 | `sigmoid(e)` | `sig(e)` | N |
| S8 | `T[…, k., …] = softmax(e)`, and `lnorm` | `T[…, k, …] = softmax[k](e)` | N |
| S9 | An implicit projection | `op[J](…)` by the scope rule ([static semantics 3](static-semantics.md#3-the-scope-rule)) | N |
| S10 | `let` (a function, a lambda, an operator or a constant) | Capture-avoiding inlining | N |
| S11 | `concat(e)` into the left index `k` | The left index $`\sum_a j_a \prod_{b > a} \lvert D_{j_b} \rvert`$ | N |
| S12 | Positional elementwise operators with broadcasting | The index form, with a size-1 axis read at its only element ([static semantics 4](static-semantics.md#4-broadcasting-and-the-positional-sugar)) | N |
| S13 | `A @ B` | The cases of `numpy.matmul` | N |
| S14 | `.T` and `transpose(A, perm)` | Permuted index lists | N |
| S15 | `sum` and `max` with `axis` | `sum[…]` and `max[…]` | N |
| S16 | `einsum(spec, …)` | Accesses with the label indices, and `sum` over the labels outside the output | N |
| S17 | Positional `exp`, `relu`, `sigmoid` and `softmax(axis=)` | Elementwise calls, or `softmax[a_k]` | N |
| S18 | Slices and integer indices | `A[a + c · j]` with a fresh `j` in $`[0, \lceil (b - a) / c \rceil)`$ | N |
| S19 | The bridge `(E)[ι]` | Named axes | N |
| S20 | Unicode primes and NFC | ASCII primes and NFC | N |
| S21 | A bare score or check expression | `score[x] = e` and `ok = e` | N |

S3 to S8 have no typing premise, but they act inside expressions.
There, the arguments of a call and the elements of a list sit below `List` children.
A congruence rule for each such position would add about 80 rules.
Also, the DDL meaning of a congruence through one element of a `List` is not established at `c68bebf5`.
So these rewrites stay in the native normalizer, which runs every rewrite in any case ([section 5](#5-normalization)).
S6 also needs Boolean operands, which only the native checker confirms.

A typed native rewrite has the form of an inference rule.
For example, the matrix product of two matrices is:

```math
\frac{\Gamma \vdash A : [m, n] \qquad \Gamma \vdash B : [n, k]}
     {A \mathbin{@} B \ \leadsto\ \big(\mathrm{sum}[j](A[i, j]\, B[j, l])\big)[\,i, l\,]}\ \ (\mathsf{MatMul_{22}})
```

The Rocq module `TensorGSLT.v` proves that each rewrite keeps the denotation ([verification plan 1](verification-plan.md#1-formal-models)).

## 4. The core

The core uses the constructors of [section 2](#2-the-tensorlogic-module) minus the derived ones.
A core term is in normal form when six invariants hold.

1. The statements of a program form a sorted bag.
2. Every chain of joins (`EJuxt`) and every chain of sums (`EAdd`) is nested to the left and sorted.
3. Parentheses (`AParen`) occur only where the precedence of [syntax 2.2](syntax.md#22-precedence-and-associativity) needs them.
4. Every index that the left side does not name is bound by an explicit projection.
5. Every bound name has its canonical spelling, `_0`, `_1` and so on, in order of first occurrence.
6. Every exact constant is folded. A call of a correctly rounded function is never folded, because a folded value would make the NF depend on the precision.

So an NF is again a term of the grammar, F1r3lang can display it with `rho:mettail:flt:display`, and it reflects with the same fingerprint.

## 5. Normalization

### 5.1 The pipeline

The native normalizer computes the NF in five deterministic phases, after the decoder of [section 6](#6-the-reflection-of-tl-terms).
Each phase is a function of its input, so the NF is a function of the term and the signature.

```text
NORMALIZE(t, sig):
  RESOLVE      ▷ check the token texts (section 2.1); bind the names and the scopes;
               ▷ NFC and UTS 39; literal kinds; keywords; call signatures
  INLINE       ▷ expand the let definitions in definition order; non-recursive, so it ends
  ELABORATE    ▷ rewrite S1 to S8 and S11 to S21, innermost first; ranks come from sig or
               ▷ from declarations, else the phase fails with RankUnknown
  SCOPE        ▷ S9: make every implicit projection explicit (the scope rule)
  CANON        ▷ fold exact constants; remove the parentheses that the precedence makes
               ▷ redundant; sort the commutative nodes by name-independent keys; refine the
               ▷ bound variables by color; break any residual tie by the exhaustive minimum,
               ▷ bounded by the static limits; rename to _0, _1, … in order of first
               ▷ occurrence; sort the statements
```

Figure TL13 shows the pipeline with the exit of each phase.

![Activity diagram of the normalization pipeline. A reflected term enters DECODE, which can exit with NotATensorTerm or UnknownTensorLanguage. RESOLVE checks the token texts, binds names and checks Unicode and call signatures, with exits for LiteralKind, ConfusableIdentifier, UnknownTensor, AmbiguousName and ArityMismatch. INLINE expands the let definitions, with an exit for InlineTooLarge. ELABORATE rewrites the sugar into the core, with exits for BroadcastMismatch, MatmulMismatch and RankUnknown. SCOPE places the implicit projections. CANON folds constants, removes redundant parentheses, sorts the commutative nodes, refines and renames the bound variables and sorts the statements. The result is the normal form, which the static checks receive next.](../diagrams/46-tl-normalization-pipeline.svg)

*Figure TL13. Source: [diagrams/46-tl-normalization-pipeline.puml](../diagrams/46-tl-normalization-pipeline.puml), rendered by [diagrams/render.sh](../diagrams/render.sh).*

### 5.2 Why the normal form is unique

Let $`E_{TL}`$ be the equational theory of TL.
It has ten groups of equations.

| Id | Equations |
|---|---|
| E1 | Each sugar form equals its rewrite ([section 3](#3-the-sugar-and-its-rewrites)). |
| E2 | `\|` is commutative and associative. |
| E3 | A parenthesized subterm equals the subterm wherever the precedence allows the subterm without parentheses. |
| E4 | The join is commutative and associative. |
| E5 | The sum is commutative and associative. |
| E6 | Bound indices can be renamed. |
| E7 | `new` names can be renamed. |
| E8 | A `let` equals its expansion. |
| E9 | An exact constant expression equals its value. |
| E10 | An identifier equals its NFC form with ASCII primes. |

**Theorem (normal form).** For all TL terms $`t`$ and $`t'`$ under one signature:

```math
t \approx_{E_{TL}} t' \;\Rightarrow\; \mathrm{NF}(t) = \mathrm{NF}(t') \qquad \text{and} \qquad [\![\mathrm{NF}(t)]\!] = [\![t]\!]
```

*Proof sketch.* The full proof is the target of `TensorGSLT.v` ([verification plan 1](verification-plan.md#1-formal-models)).

1. The sugar rules form a term rewriting system in which each left side is linear: no variable occurs twice in it.
2. No two left sides overlap. The root of each left side is a sugar constructor. Below its root, a left side holds only variables and constructors that are the root of no left side. `RuleFold` folds from the right end, so it does not overlap itself.
3. A linear system without overlaps is orthogonal, and an orthogonal system is confluent ([Hue80](#ref-hue80)).
4. The system terminates, by a polynomial interpretation that weighs every sugar constructor above every core constructor ([BN98](#ref-bn98)). So the innermost strategy of the implementation is total.
5. A rewrite below a typing premise keeps that premise (subject reduction), so the conditional rules stay joinable.
6. RESOLVE, INLINE, SCOPE and CANON are functions. ELABORATE and SCOPE apply E1, INLINE applies E8, and CANON quotients E2 to E7, E9 and E10.

So equal computations normalize to one term, and every rewrite keeps the denotation. ∎

## 6. The reflection of TL terms

F1r3lang reflects a guest term to a Rholang `Par` (mettail-rust at `c68bebf5`, `rholang-codegen/src/rho_net_lower.rs`).

- A constructor `C(a1, …, an)` reflects to `EList[GPrivate("mettail.term.{F_TL}.C"), GPrivate("mettail.term.{F_TL}.^gnd" or "^nog"), ⟦a1⟧, …, ⟦an⟧]`, where `F_TL` is the fingerprint of TL (lines 1762 to 1793 and 4209 to 4232).
- The children of a `List` sort are positional.
- The `HashBag` of `PComp` reflects to a bag of sends `@"ac:…"!(⟦s⟧)`, which the canonical order of `Par` makes canonical (lines 3167 to 3172).
- A token with a `String` carrier becomes a literal leaf whose tag holds the text (point 3 of [section 2.3](#23-the-installed-rewrites)).

For example, `Y = step(W[i] X[i])` reflects to a nest of lists headed by the tags of `SEq`, `LBare`, `OpSum`, `ACall`, `FStep`, `ArgE`, `EJuxt`, `EIdx` and `AName`.

The TL decoder splits each tag with `parse_reflected_tag`, the one inverse of the tag that F1r3lang defines (line 1820).
It validates every arity, sort and leaf, and it fails closed with `NotATensorTerm`.
The tags are unforgeable `GPrivate` names, but a program can still build the children by hand.
So the decoder trusts nothing that it has not checked.

## 7. No change to the node grammar

TL terms are values inside the query and params fields of a bind and of a send ([decision 112](../decision-register.md#decision-112)). *Changed by [decision 112](../decision-register.md#decision-112): before, they were values inside the `QUERY` and `PARAMS` tuples of decision 18.*
The fields already accept any `Par` (`models/src/rust/reified/encoding.rs:663-683`), so the node grammar does not change.
One validator changes: the literal check of `threshold` must know the query group, because its range relaxes when the group holds `score` ([decision 56](../decision-register.md#decision-56), `models/src/rust/reified/validation.rs:172-226`).

## 8. Content identity, installation and versioning

- **Fingerprint.** $`\mathcal{F}_{TL}`$ is `mettail-langdef-v2:` followed by the BLAKE3 hash of the canonical TL definition, as F1r3lang computes it for every language.
- **Allow-list.** The node ships the module source, and the evaluator pins the allowed fingerprints. A test recomputes $`\mathcal{F}_{TL}`$ from the source. Any other fingerprint fails with `UnknownTensorLanguage`.
- **Publication.** The module is published in the Versioned Registry as `rho:lib:1.0.0:<publisher>:tensorlogic:1.0.0` ([VRF25](#ref-vrf25)), and a program installs it with `rho:mettail:install`.
- **Versions.** A minor version only adds constructors and gets a new allowed fingerprint. To accept a new fingerprint is visible to consensus, so it ships with a node release. A major version gets a new checker ABI.

Figure TL14 shows the installation and the identity.

![Component diagram of the installation and the identity of TensorLogic. The node release ships the TensorLogic module source and the pinned fingerprint allow-list of the TL engine. The Versioned Registry publishes the module under rho:lib with a version. A program looks the module up and installs it with rho:mettail:install, and the installer computes the fingerprint F of the canonical definition and returns the opaque handle tl. FLT terms written with the handle reflect to Par values whose tags carry F. The TL engine accepts a term only when F is in its allow-list. A TensorCheck theory stores its content identity as compact bytes in the config of a space, and its canonical normal form as a content record under KIND T.](../diagrams/48-tl-identity-install.svg)

*Figure TL14. Source: [diagrams/48-tl-identity-install.puml](../diagrams/48-tl-identity-install.puml), rendered by [diagrams/render.sh](../diagrams/render.sh).*

## 9. The capability rho:lang:vector

[Decision 53](../decision-register.md#decision-53) puts TL-1 programs behind the existing capability `rho:lang:vector`, and [decision 17](../decision-register.md#decision-17) admits native engines only as capabilities with a URN.
So TL extends that capability and adds no URN (open question K10).

- **Address.** The capability keeps the channel `[0xE0, 0x23]`, the body ref 235 and the packed arity 2, `[ret, [op, x…]]` (`rholang/src/rust/interpreter/reified/system_channels.rs:427-434`). It is a deterministic call, and it is not recorded.
- **Replies.** The reply comes first: `[true, …]` or `[false, code, detail]` ([decision 57](../decision-register.md#decision-57)). The Agents sugar `vec!eval(…)` applies ([AGF25](#ref-agf25)).

| Method | Inputs | Output |
|---|---|---|
| `eval` | A `tl:Program`, the map of tensors, and options `{"precision": 8..256, "as": "auto" \| "exact" \| "f64"}` | A map from each query, as the NF displays it, to its value |
| `normalize` | A term of any TL category, and a signature: a map from names to shapes | The reflected NF |
| `check` | A term and a signature | The inferred signature and the demand bound $`B`$ |
| `demand` | A term, a signature and options | `[ops, bytes, backing]` |

- **The output kind `auto`.** An integer result of integer inputs is a `GInt` or a `GBigInt`. Another result is a `GBigRat`, unless an input is a float, in which case it is a `GDouble` of $`\mathrm{RN}_{53}`$. This extends the rule of the widest kind of [decision 66](../decision-register.md#decision-66).
- **Errors.** The codes of [static semantics 12](static-semantics.md#12-diagnostics), and `UnknownMethod` and `BadArgument`.
- **Costs.** Every phase reserves on the budget of the deploy, as the existing operations of the capability do (`rholang/src/rust/interpreter/reified/system_processes.rs:448-468`).

## 10. Admission theories

A TL check term can serve as the theory of a space, through the theory seam of [decision 77](../decision-register.md#decision-77).

- **Form.** `free TensorCheck(tl, tl:Check{ v : [D] in { min[d](v[d] >= 0) } }, 64)` admits only vectors with no negative element. The first argument is the handle, as the theory seam requires after the merge of F1r3lang.
- **Identity bytes** ([decision 74](../decision-register.md#decision-74)):

| Field | Value |
|---|---|
| version | 1 |
| languageFingerprint | $`\mathcal{F}_{TL}`$ |
| category, categoryName | `"Check"` |
| theoremId | `"tl:check:"` followed by the hexadecimal digits of $`h`$ |
| checkerAbi | `"rrs:tensorlogic:v1"`. It pins the semantics and the Unicode data. |
| limitProfile | Work bounded by $`B`$ for the declared signature, and evidence 0 |
| args | `[h, p]` |

Here $`h`$ is the Blake2b-256 hash of the canonical bytes of the reflected NF, and $`p`$ is the precision.
The content record of the NF is `E0‖T‖h`, under the reserved KIND `T` (open question K8).

- **Verdicts.** `Proven` holds exactly when `ok` is 1. `Refuted` holds when `ok` is 0 or when the value does not decode to the signature. `Undetermined` holds on a limit or on `PrecisionExhausted`.
- **Costs.** The check reserves its work before it reads the value, as the theory seam requires.

## 11. The OSLF view

The installed rewrites of TL only remove sugar.
OSLF modalities over them could express predicates such as "the term normalizes to a term with the property φ" ([SM16](#ref-sm16), [WS22](#ref-ws22)).
The first version delivers this value in two simpler ways: structural FLT patterns and guards on TL terms, and native typed admission through `TensorCheck`.
Native types for shapes, and the satisfaction surface `t |= φ` of the MeTTaIL FIPS, are future work for the owner of F1r3lang.

## References

<a id="ref-agf25"></a>[AGF25] M. Stay, "Agents," F1r3fly Improvement Proposal, 2025-08-20. FIPS repository, approved/2025-08-20-Agents.

<a id="ref-bn98"></a>[BN98] F. Baader and T. Nipkow, *Term Rewriting and All That*, Cambridge University Press, 1998. [doi:10.1017/CBO9781139172752](https://doi.org/10.1017/CBO9781139172752)

<a id="ref-hue80"></a>[Hue80] G. Huet, "Confluent reductions: abstract properties and applications to term rewriting systems," *Journal of the ACM*, vol. 27, no. 4, pp. 797–821, 1980. [doi:10.1145/322217.322230](https://doi.org/10.1145/322217.322230)

<a id="ref-sm16"></a>[SM16] M. Stay and L. G. Meredith, "Logic as a distributive law," arXiv:1610.02247, 2016. [doi:10.48550/arXiv.1610.02247](https://doi.org/10.48550/arXiv.1610.02247)

<a id="ref-vrf25"></a>[VRF25] "Versioned Registry," F1r3fly Improvement Proposal, 2025-09-16. FIPS repository, approved/2025-09-16-Versioned-Registry.

<a id="ref-ws22"></a>[WS22] C. Williams and M. Stay, "Native Type Theory," *Electronic Proceedings in Theoretical Computer Science*, vol. 372, pp. 116–132, 2022. [doi:10.4204/EPTCS.372.9](https://doi.org/10.4204/EPTCS.372.9)
