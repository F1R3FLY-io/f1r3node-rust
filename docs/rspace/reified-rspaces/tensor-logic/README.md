# TensorLogic for Reified RSpaces

TensorLogic (TL) is a small language for tensor equations and logic rules ([Dom25](#ref-dom25)).
This directory holds the design of its syntax, its semantics and its GSLT for the Reified RSpaces campaign.
A VectorDB space uses TL to score records, and the capability `rho:lang:vector` uses TL to evaluate whole programs.

**Status.** This design is a proposal for phase P12.
P12 starts only after the F1r3lang readiness signal SIG-F and the merge of F1r3lang ([decision 65](../decision-register.md#decision-65)).
[Decision 64](../decision-register.md#decision-64) requires the approval of the user for the grammar before any code.
The open questions of the design are in [section 3 of the design rationale](design-rationale.md#3-open-questions).
No code of this design exists yet, and no decision of the register records its approval.

*Changed by [decision 89](../decision-register.md#decision-89): the base removed host-work metering, and Reified RSpaces removed its own. The figures of this proposal show no meter. The text still names host work, a meter, a demand bound and reservations, as the proposal had them. [Open question K17](design-rationale.md#3-open-questions) restates that cost model before any code.*

## Contents

1. [The design in five points](#1-the-design-in-five-points)
2. [Reading order](#2-reading-order)
3. [The documents](#3-the-documents)
4. [The findings of the design pass](#4-the-findings-of-the-design-pass)
5. [The decisions that bind the design](#5-the-decisions-that-bind-the-design)
6. [How the design was made](#6-how-the-design-was-made)

The [references](#references) follow the last section.

## 1. The design in five points

1. TL is a GSLT that a node installs in F1r3lang. Its terms are foreign-language terms (FLTs) such as `tl:Program{…}`, `tl:Score{…}` and `tl:Check{…}` ([syntax 3](syntax.md#3-the-embedding-in-rholang-14)).
2. The surface is the index notation of the paper, with a functional layer (`let`, `x => e`), a Rholang layer (`|`, `new … in {…}`) and numpy-like sugar ([syntax 2](syntax.md#2-the-grammar)).
3. A native normalizer reduces every surface term to one canonical core term, the normal form (NF). The NF is the paper notation with explicit projections such as `sum[j](…)` ([GSLT 5](gslt.md#5-normalization)).
4. A metered native evaluator computes the NF in exact rational arithmetic, with correctly rounded transcendental functions ([dynamic semantics](dynamic-semantics.md)).
5. The evaluator runs inside the VectorDB agent for scores, and behind `rho:lang:vector` for programs. Admission predicates (`tl:Check`) enter spaces through the theory seam ([integration](integration.md)).

Figure TL1 shows the components from the source text to the stored records.

![Component diagram of TensorLogic from end to end. A Rholang 1.4 deploy holds FLT terms of TensorLogic. The F1r3lang frontend lexes the FLT body with the installed TensorLogic language, parses it with the GSLT grammar and reflects it to a Par, whose tags carry the fingerprint of the language. The reducer passes the reflected term as a value, inside a VectorDB query or as an argument of rho:lang:vector. The TL engine decodes the term, normalizes it, checks it and evaluates it with the exact core of the tensor module and the correctly rounded functions, and no meter covers that work. The VectorDB agent scores records in RSpace with a tl:Score term, and the capability rho:lang:vector evaluates a tl:Program. The theory seam admits values with a tl:Check term. The registry publishes the language module, and the installer binds the handle.](../diagrams/35-tl-components.svg)

*Figure TL1. Source: [diagrams/35-tl-components.puml](../diagrams/35-tl-components.puml), rendered by [diagrams/render.sh](../diagrams/render.sh).*

## 2. Reading order

A reader new to TL reads the documents in this order.

1. [Concepts](concepts.md) defines tensors, indices, joins, projections, rules and the other terms. Every later document uses these terms.
2. [Syntax](syntax.md) gives the lexical rules, the grammar, the embedding in Rholang 1.4 and many examples.
3. [Static semantics](static-semantics.md) gives the shapes, the typing rules, the scope rule and the static limits.
4. [Dynamic semantics](dynamic-semantics.md) gives the meaning of each construct, the exact arithmetic and the evaluation algorithm.
5. [The GSLT](gslt.md) gives the TL module in the definition language of F1r3lang, the rewrites of the sugar and the normal form.
6. [Integration](integration.md) connects TL to VectorDB spaces, to the capability and to the theory seam.
7. [Verification plan](verification-plan.md) lists the models, proofs and tests that P12 must deliver.
8. [Design rationale](design-rationale.md) reviews the legacy support, the rejected alternatives, the open questions and the work order.

## 3. The documents

| Document | Content |
|---|---|
| [concepts.md](concepts.md) | The terms of TL, with the intuition of joins, projections and rules |
| [syntax.md](syntax.md) | Lexical structure, EBNF grammar, precedence, ambiguity rules, FLT embedding, value ABI, worked examples |
| [static-semantics.md](static-semantics.md) | Sorts, shape inference, typing rules, the scope rule, broadcasting, safety, recursion classes, the demand bound, diagnostics |
| [dynamic-semantics.md](dynamic-semantics.md) | Denotation, exact arithmetic, correct rounding, evaluation, determinism, consensus state |
| [gslt.md](gslt.md) | The TL module, its sugar rewrites, the normal form, reflection, identity, the capability, admission theories |
| [integration.md](integration.md) | Score queries, rules, embedding reasoning, `where` guards, replay and merges, costs, errors |
| [verification-plan.md](verification-plan.md) | Rocq, Z3, Sage and rewriting checks, mutants, properties and tests |
| [design-rationale.md](design-rationale.md) | Legacy review, alternatives, open questions, missing F1r3lang facilities, implementation order |

## 4. The findings of the design pass

The design pass checked its premises in the code of this tree and of the F1r3lang branch that the node integrates.
Seven findings change or qualify earlier records.

| Id | Finding | Consequence |
|---|---|---|
| F1 | The correction of decision 64 says that a construction hole needs an enclosing FLT pattern. That holds on `mettail-rust` at `7394815c`. The node integrates `mettail-module-dev/mettail-rust` at `c68bebf5`, where a hole takes any lexical binding. | The decided data path stays: reserved names and the param `tensors`. Holes as a second path are open question K1. |
| F2 | The line numbers that decision 62 cites belong to two checkouts. | This design cites `c68bebf5` and states the checkout of each citation. |
| F3 | An installed F1r3lang theory can call only six intrinsics, with no rational arithmetic. Its handler ABI has a fixed cost. | TL evaluates natively, as decision 62 states, and never through a handler. |
| F4 | A crafted embedding can force `PrecisionExhausted` while a query scores a record. | Open question K2 recommends that such a record becomes ineligible. |
| F5 | The VectorDB proofs carry over to TL scores only when a score depends on its own record. | Scores must be record-local ([static semantics 9](static-semantics.md#9-record-locality-of-scores)). |
| F6 | Source text admits a query only on a linear bind. | Peek and persistent score queries wait for the Rholang 1.4 productions (K11). |
| F7 | Decision 53 excludes virtual indices and recursion in a COMM, and decision 64 has both in the core. | Both stay out of scores and checks, and programs may use them (K16). |

## 5. The decisions that bind the design

| Decision | What it fixes |
|---|---|
| [52](../decision-register.md#decision-52) | The approved P6 study: the fragments TL-0 to TL-3, exact arithmetic, correct rounding, rules as joins, the vocabulary |
| [53](../decision-register.md#decision-53) | The fragment of TL for VectorDB and its exclusions |
| [54](../decision-register.md#decision-54) | Exact arithmetic for every consensus decision, and correctly rounded functions at a declared precision |
| [62](../decision-register.md#decision-62) | TL equations are FLT terms of a TensorLogic GSLT, with no string syntax |
| [63](../decision-register.md#decision-63) | An index-notation core with numpy-like sugar, and one normal form |
| [64](../decision-register.md#decision-64) | A paper-first, functional, Rholang-flavored surface, ASCII built-ins and checked Unicode names |
| [65](../decision-register.md#decision-65) | The whole language waits for F1r3lang (P12), and it does not block VectorDB |
| [37](../decision-register.md#decision-37), [40](../decision-register.md#decision-40), [41](../decision-register.md#decision-41), [69](../decision-register.md#decision-69), [74](../decision-register.md#decision-74), [77](../decision-register.md#decision-77) | Selection with `where` guards, metering, tie-breaks, native execution only, compact theory identity, GSLT typing |

## 6. How the design was made

A design agent read the paper, the decision register, the P6 study, the code of this tree, the F1r3lang code and the FIPS documents.
The main session then checked the load-bearing claims in the code.
These claims include the dependency of the node on `mettail-module-dev` at `c68bebf5` and the binding of holes.
They also include the six intrinsics, the FLT lexer modes and the DDL constructs.
The other claims are `rho:lang:vector` at `[0xE0, 0x23]`, the free op `0x26`, the VectorDB proof hypotheses and the number ingress.

While it wrote these documents, the main session also read the definition language and the regex compiler of F1r3lang at `c68bebf5`.
That reading gave the token declarations of [GSLT 2.1](gslt.md#21-the-tokens) and kept the sugar inside expressions native ([GSLT 3](gslt.md#3-the-sugar-and-its-rewrites)).
It also completed the semantics with the call signatures ([static semantics 2.3](static-semantics.md#23-calls)) and the built-in functions ([dynamic semantics 2.6](dynamic-semantics.md#26-built-in-functions)).
Every DOI of the references resolves at doi.org.

## References

<a id="ref-dom25"></a>[Dom25] P. Domingos, "Tensor Logic: The Language of AI," arXiv:2510.12269v3, 2025. [doi:10.48550/arXiv.2510.12269](https://doi.org/10.48550/arXiv.2510.12269)
