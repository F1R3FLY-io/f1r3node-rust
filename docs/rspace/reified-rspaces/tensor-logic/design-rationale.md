# TensorLogic design rationale

This document explains why the design of TensorLogic (TL) has its form.
It reviews the legacy support and lists the rejected alternatives.
It states the open questions and the missing facilities of F1r3lang.
It also gives the order of the implementation and the changes to the approved P6 study.
The terms are defined in [concepts](concepts.md).

## Contents

1. [The legacy support](#1-the-legacy-support)
2. [Alternatives considered and rejected](#2-alternatives-considered-and-rejected)
3. [Open questions](#3-open-questions)
4. [Missing F1r3lang facilities](#4-missing-f1r3lang-facilities)
5. [Implementation order](#5-implementation-order)
6. [Changes to the approved P6 study](#6-changes-to-the-approved-p6-study)

## 1. The legacy support

The legacy feature branch had a `tensor/` module and string operations behind a system process.
The user was not satisfied with that support ([decision 52](../decision-register.md#decision-52)), so this design reviews it and ports none of it.
The citations below are from the legacy checkout (f1r3node-reified-rspaces at `ff479042`).

| Legacy defect | Evidence | How TL avoids it |
|---|---|---|
| No tensor equation, no join, no projection and no rules. `einsum` is a `match` over fixed strings, and any other string panics. | Lines 468 to 519 of `rholang/src/rust/interpreter/tensor/operations.rs` | The equation is the only construct, and `einsum` is general sugar (S16). |
| `assert!` panics that user input can reach | Lines 98, 213, 258, 696, 774 and 837 of `operations.rs` | Every check fails closed with a typed code. |
| `f32` arithmetic, parallel evaluation above a size threshold, and `partial_cmp(…).unwrap_or(Equal)` | Lines 68 to 107 and 438 of `operations.rs` | Exact, sequential arithmetic, and total orders with the tie key $`\kappa`$ |
| Lossy outputs: values cut to 0 or 1 at 0.5, floats as comma-separated strings, a threshold scale of integers from 0 to 100 | Lines 499 to 512, 2505 to 2510, 2453 to 2457 and 2010 to 2036 of `rholang/src/rust/interpreter/system_processes.rs` | The exact value ABI, the option `as`, and rational thresholds |
| The temperature is read in the wrong direction: a comment calls a larger temperature sharper. Only integer temperatures pass, and the temperature 0 panics. | Lines 2850 to 2856 of `system_processes.rs`, and line 98 of `operations.rs` | $`\sigma_T`$ as in section 5 of the paper ([Dom25](#ref-dom25)), with rational temperatures and a defined temperature 0 |
| Operation names as strings, and the reply channel last | Lines 2841 to 2844 of `system_processes.rs` | FLT terms, and the reply first ([decision 57](../decision-register.md#decision-57)) |
| Tests compare floats within tolerances, such as `(t1[i] - s[i]).abs() < 0.001` | Lines 855 to 870 of `operations.rs` | Exact golden values, Sage reference values and Rocq models |

## 2. Alternatives considered and rejected

| Alternative | Why the design rejects it |
|---|---|
| Equations as strings (TEL) | [Decision 62](../decision-register.md#decision-62) replaces them with FLT terms. |
| A positional numpy language only | It loses the logic reading of named indices ([decision 63](../decision-register.md#decision-63)). |
| Datalog only | It has no numeric tensors. |
| A flat core in A-normal form, with an auxiliary tensor for each subterm | It loses the fusion of projections and stores every intermediate tensor. |
| Explicit summation only | It is not the notation of the paper. The NF keeps it, and the surface allows it. |
| Evaluation through the handler ABI of F1r3lang | The handler cost is affine and fixed in advance, so it does not meter the work (missing facility MF2). |
| All sugar as installed F1r3lang rewrites | The typing premises and the arithmetic intrinsics are missing, and the sugar inside expressions needs a congruence rule for each position below a `List` ([GSLT 3](gslt.md#3-the-sugar-and-its-rewrites)). |
| IEEE floating-point semantics | They break [decision 54](../decision-register.md#decision-54). |
| A join that is strict in $`\bot`$ | Dense and sparse evaluation would differ, so the metering would depend on the evaluation strategy. |
| Statements separated by line breaks | Juxtaposition would join terms across a line break. |
| Signed numeral tokens, as the `Int` token of Rholang 1.4 | Juxtaposition makes `a -1` ambiguous. |
| Normalizing the Unicode minus U+2212 to `-` | It breaks the rule that built-in symbols are ASCII (open question K5). |
| A higher-order `let` | It needs a proof of strong normalization, and its inlining can grow without bound. |
| A new URN for TL-1 | [Decision 53](../decision-register.md#decision-53) puts TL-1 behind `rho:lang:vector`. |
| Lambda parameters instead of reserved names in scores | [Decision 64](../decision-register.md#decision-64) binds reserved names. |
| Identifiers that mix scripts | The protection against look-alike names is weaker (open question K6). |

## 3. Open questions

The user answers each question before any code ([decision 64](../decision-register.md#decision-64)).
Each entry gives the options, the recommendation, and what the other options leave out.

- **K0. The grammar.** Approve the lexical rules and the grammar of [syntax](syntax.md), with the answers to K3 to K6. Recommendation: approve.
- **K1. Holes as a data path** (finding F1).
  - (a) Only the param `tensors` in the first version.
  - (b) Also holes such as `${w : Expr}`, filled from Rholang bindings and decoded with the value ABI.
  - (c) Holes only.
  - Recommendation: (a) now and (b) later. The fill of a hole happens at COMM time without a category check, and that path is not verified yet. Option (c) cannot carry the records of a score, which the agent supplies.
- **K2. `PrecisionExhausted` while a query scores a record** (finding F4).
  - (a) Keep the approved abort of the deploy.
  - (b) Make the record ineligible.
  - Recommendation: (b). Under (a), one crafted embedding can make every score query of a collection abort.
- **K3. Additions beyond the lists of decisions 63 and 64.** The candidates are `min=`, `maj=` (FIPS line 261), `lnorm`, `sin`, `cos` and `rsqrt` (Table 2 of the paper), and the positional reductions `min` and `mean`. Recommendation: add all except the positional reductions `min` and `mean`, which `min[…]` and `avg[…]` already express.
- **K4. An unsuffixed decimal literal.**
  - (a) The exact decimal value.
  - (b) The `f64` value, as the Numeric Types FIPS defines it.
  - Recommendation: (a), because decision 63 excludes `float64` rounding. Option (b) would make `0.1` differ from $`1/10`$.
- **K5. A confusable operator character, such as U+2212.**
  - (a) Reject it, with the ASCII spelling as the fix.
  - (b) Normalize it to the ASCII operator.
  - Recommendation: (a). Option (b) admits non-ASCII spellings of built-in operators.
- **K6. The level of identifier security.**
  - (a) A single script, with the General Security Profile.
  - (b) Highly Restrictive.
  - (c) Moderately Restrictive.
  - Recommendation: (a). Option (b) rejects common mathematical names, and option (c) admits mixed scripts.
- **K7. The sigmoid at the temperature 0.**
  - (a) The approved `step`.
  - (b) The limit $`H_{1/2}`$, which is one half at 0.
  - Recommendation: (b). It keeps the threshold property at the temperature 0, and it agrees with `softmax` at the temperature 0 ([dynamic semantics 2.5](dynamic-semantics.md#25-temperature)). Option (a) keeps the approved value but gives 0 at the point 0.
- **K8. The content of an admission theory.**
  - (a) A content record `E0‖T‖h`, with the hash in the args.
  - (b) The term inline in the args.
  - Recommendation: (a). Every space operation reads the config, so inline args would make every operation more expensive ([decision 74](../decision-register.md#decision-74)).
- **K9. A snapshot of several relations for TL-2b.**
  - (a) The patterns of [integration 4](integration.md#4-rules-tl-2): one relation value in a cell, or one list read.
  - (b) A new query key `rules`.
  - (c) Lifting `ListModeInJoin`.
  - Recommendation: (a). Options (b) and (c) change approved P6 rules.
- **K10. The capability for TL-1.**
  - (a) Extend `rho:lang:vector`.
  - (b) A new URN `rho:lang:tensor` at `[0xE0, 0x26]`, with the body ref 238.
  - Recommendation: (a), as decision 53 states.
- **K11. Score queries on peeks and persistent binds** (finding F6). The Rholang 1.4 productions of P12 should accept a query on every kind of bind. Recommendation: yes.
- **K12. The delivered TL scores.**
  - (a) $`\mathrm{RN}_{53}`$, as for the built-in metrics.
  - (b) An exact option.
  - Recommendation: (a). The exact value already decides the selection, and the delivery only reports it.
- **K13. The binding of the handle.** Install the module from the registry, or bind it with `free TensorLogic()`. The user decides this together with the owner of F1r3lang ([F1r3lang integration 6](../f1r3lang-integration.md#6-open-items-for-phase-p12)).
- **K14. The Unicode version.** Pin it to the Unicode version of the normalization and security crates of the node. Record it in the checker ABI. Recommendation: yes.
- **K15. The installed rewrites.** Install the rewrites of S1 and S2, so that F1r3lang agrees with the native normalizer. Check the agreement with the test `prop_tl_installed_rewrites_agree`. Recommendation: yes. The sugar inside expressions stays native ([GSLT 3](gslt.md#3-the-sugar-and-its-rewrites)).
- **K16. Virtual indices and recursion** (finding F7). Confirm that both stay in `tl:Program` and out of scores and checks. Recommendation: confirm.
- **K17. The cost model after [decision 89](../decision-register.md#decision-89).** The host-work budget that this proposal uses no longer exists. Restate the cost model in terms of phlo before any code.

## 4. Missing F1r3lang facilities

The owner of F1r3lang can add these facilities.
TL works without each one, with the workaround in the last column.

| Id | Facility | Why TL needs it | Workaround |
|---|---|---|---|
| MF1 | Arithmetic and comparison intrinsics | An installed theory has six intrinsics, with no rational arithmetic, no comparison and no division (lines 51 to 56 of `mettail-elab/src/theory_compile.rs`, mettail-rust at `c68bebf5`). | TL evaluates natively ([decision 62](../decision-register.md#decision-62)). |
| MF2 | A metered handler ABI | A handler pays an affine cost that is fixed in advance (lines 70 to 88 of `grammar-core/src/capability.rs`, mettail-rust at `c68bebf5`). [Decision 40](../decision-register.md#decision-40) requires a reservation before each step of work. | TL never evaluates through a handler. |
| MF3 | The Reified query productions in Rholang 1.4 | A score query needs `~ (…; score = tl:Score{…})` in Rholang 1.4. | P12 adds the productions together with the owner. |
| MF4 | A parser preference for rule R2 | `Ident (` must be a call, but the grammar alone allows two readings. | The token `Ident(` without a space carries the call. |
| MF5 | A hook for static checks when F1r3lang builds an FLT, with spans | TL errors would then point at the source text. | Diagnostics name tensors, indices, shapes and axes ([static semantics 12](static-semantics.md#12-diagnostics)). |
| MF6 | Comments in guest languages | TL comments are `//` and `/* … */`. | Comments go in the Rholang around the FLT. |
| MF7 | Contextual keywords in guest languages | `T` after a dot, and `bool` and `sym` in a shape, are keywords only in context. | These words become reserved. |

## 5. Implementation order

P12 starts only after the readiness signal SIG-F and the merge of F1r3lang ([decision 65](../decision-register.md#decision-65)).
The steps below follow the dependencies of the design.

1. Parse the TL module with the DDL parser of F1r3lang. Record the answers to the four open points of [GSLT 2.3](gslt.md#23-the-installed-rewrites).
2. Add the frontend-neutral TL syntax tree and the value ABI in `models/src/rust/reified/tensor/`.
3. Add the correctly rounded functions and the Ziv loop in `rholang/src/rust/interpreter/tensor/cr.rs`, with their costs.
4. Add the normalizer, the checker and the demand bound, and then the evaluator, in `rholang/src/rust/interpreter/tensor/tl/`.
5. Write the Rocq, Z3, Sage and rewriting legs of the [verification plan](verification-plan.md), in parallel with steps 2 to 4.
6. Add the GSLT module source, the pin of the fingerprint and the decoder of reflected terms.
7. Add the methods of `rho:lang:vector`.
8. Add the VectorDB keys `score`, `tensors` and `precision`.
9. Add `TensorCheck` on the merged theory seam.
10. Certify in native play and native replay at the limits of protocol 6, and complete the documents and the diagrams.

## 6. Changes to the approved P6 study

The P6 study of [decision 52](../decision-register.md#decision-52) designed the TL fragment with equations as strings.
Decisions 62 to 65 moved the language to FLT terms at P12.
This design keeps the semantics of the study and changes the points below.

| Topic | The P6 study | This design | Reason |
|---|---|---|---|
| Surface | Strings in `score` and in `eval` | FLT terms of the TensorLogic GSLT | [Decision 62](../decision-register.md#decision-62) |
| Vector functions | `xor`, `dot`, `dist2`, `dist`, `l1`, `hamming` and `jaccard` as built-ins | Index notation, for example `a + b - 2 a b` for `xor` and `sum[i](a[i] b[i])` for `dot` | One core with no vector keywords. The VectorDB metrics keep their names in `metric`. |
| The normalized index | A dot inside the argument, as `rows[x., 0]` | A dot on the left side, as `Comp[p, p'.]` in the paper, or the index in `softmax[k]` | The notation of the paper ([Dom25](#ref-dom25), p. 10) |
| `softmax` at the temperature 0 | Not defined | The limit: equal weights on the largest logits | [Dynamic semantics 2.5](dynamic-semantics.md#25-temperature) |
| The keys `score`, `tensors` and `precision` | P6 keys | P12 keys | Decisions [65](../decision-register.md#decision-65) and [66](../decision-register.md#decision-66) |
| TL-2b | A join of two list reads | One snapshot, because a list-mode bind in a join fails with `ListModeInJoin` | [Decision 66](../decision-register.md#decision-66) and open question K9 |

The study keeps its other rules: exact arithmetic, the correctly rounded functions and the scope rule.
It also keeps `hardsig` with the slope of one sixth, and $`\sigma_0`$ as `step` unless K7 changes it.

## References

<a id="ref-dom25"></a>[Dom25] P. Domingos, "Tensor Logic: The Language of AI," arXiv:2510.12269v3, 2025. [doi:10.48550/arXiv.2510.12269](https://doi.org/10.48550/arXiv.2510.12269)
