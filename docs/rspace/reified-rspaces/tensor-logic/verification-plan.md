# TensorLogic verification plan

This document lists the evidence that phase P12 must deliver for TensorLogic (TL).
The evidence follows the six acceptance categories of the campaign.
They are formal models, formal verification with negative controls, invariants as property tests, a complete implementation, thorough tests and documentation.
The terms are defined in [concepts](concepts.md).

Figure TL20 shows how the sources, the checks and the gates produce the evidence of each category.

![Verification pipeline of TensorLogic as four columns from the left to the right. Sources: the TL engine code, the GSLT module source, the Rocq models TensorLogic.v and TensorGSLT.v, the TLA+ model VectorDb.tla with its score configurations, the Z3 and Sage scripts, the rewriting file tl_sugar.trs, the invariant map and the documents. Checks: the Rocq kernel checks with closed assumptions, TLC on the safe and the Unsafe configurations, Z3, Sage, TTT2 for termination and CSI for confluence, the property-based tests, the deterministic suites with native play and native replay, the mutants that must fail, and the document checks. Gates: check-reified-rspaces-proofs.sh, check-reified-rspaces-tla-invariants.sh, the Z3, Sage and rewriting legs, cargo test, the checkers and doc_lint.py with render.sh. Acceptance categories: modeled, verified, invariants as properties, complete, tested and documented. Arrows show which gate gives evidence of which category.](../diagrams/54-tl-verification-pipeline.svg)

*Figure TL20. Source: [diagrams/54-tl-verification-pipeline.puml](../diagrams/54-tl-verification-pipeline.puml), rendered by [diagrams/render.sh](../diagrams/render.sh).*

## Contents

1. [Formal models](#1-formal-models)
2. [Formal verification and negative controls](#2-formal-verification-and-negative-controls)
3. [Invariants as property tests](#3-invariants-as-property-tests)
4. [A complete implementation](#4-a-complete-implementation)
5. [Thorough tests](#5-thorough-tests)
6. [Documentation](#6-documentation)

## 1. Formal models

### 1.1 The rules of the models

Every model follows the rules of the campaign.

- A Rocq module has no `Admitted`, `admit`, `Axiom`, `Parameter`, `Conjecture` or `Abort`. A section hypothesis is allowed only when an instance discharges it.
- The numeric models are parametric over a decidable ordered field, with an instance in the rationals `Qc`, in the style of `Similarity.v`. No model uses IEEE floating point.
- Each TLA+ configuration starts with a `\* BOUNDS:` header, and each Unsafe configuration refutes exactly one named invariant.

### 1.2 TensorLogic.v

The module `formal/rocq/reified_rspaces/theories/TensorLogic.v` models the core and its denotation.

| Theorem | Statement | Claim |
|---|---|---|
| `denote_functional` | The denotation is a function of the term and the inputs. | [Dynamic semantics 7](dynamic-semantics.md#7-determinism-termination-and-resource-bounds) |
| `builtin_domains` | Each built-in function is strict in $`\bot`$ and gives $`\bot`$ exactly outside its domain. | [Dynamic semantics 2.6](dynamic-semantics.md#26-built-in-functions) |
| `join_perm_invariant`, `sum_perm_invariant` | A permutation of the operands of a join or a sum keeps the denotation. | E4 and E5 of [GSLT 5.2](gslt.md#52-why-the-normal-form-is-unique) |
| `program_perm_invariant` | A permutation of the statements keeps the denotation. | E2 |
| `alpha_index_invariant` | A renaming of bound indices keeps the denotation. | E6 |
| `projection_fusion`, `sum_distributes` | Nested projections fuse, and a sum distributes over a join with a factor that does not hold its index. | [Static semantics 3](static-semantics.md#3-the-scope-rule) |
| `scope_rule_paper_examples`, `spine_max_relu` | The scope rule gives the meanings of the table of static semantics 3.3. | [Static semantics 3.3](static-semantics.md#33-the-rule-on-the-examples-of-the-paper) |
| `zero_absorbs_bot` | A dense evaluation and a sparse evaluation agree. | [Dynamic semantics 1](dynamic-semantics.md#1-values-and-operations) |
| `datalog_rule_step_einsum` | A Datalog rule equals the step function of an einsum ([Dom25](#ref-dom25), p. 5). | [Concepts 5](concepts.md#5-rules-facts-and-boolean-tensors) |
| `seminaive_least_fixpoint`, `fixpoint_rounds_bound` | Semi-naive iteration reaches the least fixpoint, within the number of possible facts. | [Dynamic semantics 5](dynamic-semantics.md#5-the-evaluation-algorithm) |
| `guarded_recursion_unique` | A component of class R1 has one solution. | [Static semantics 7](static-semantics.md#7-recursion-classes-and-stratification) |
| `stratified_perfect_model` | Stratified negation gives the perfect model ([ABW88](#ref-abw88)). | [Dynamic semantics 2.3](dynamic-semantics.md#23-programs) |
| `maj_avg_props` | `maj` and `avg` meet their definitions, and a tie of `maj` gives 0. | [Dynamic semantics 2.1](dynamic-semantics.md#21-expressions) |
| `temperature_threshold` | For every $`T > 0`$, $`\sigma_T(s) \ge 1/2`$ holds exactly when $`s \ge 0`$. | [Dynamic semantics 2.5](dynamic-semantics.md#25-temperature) |
| `softmax_zero_two_logits` | At the temperature 0, `softmax` on the logits $`x`$ and 0 equals $`H_{1/2}(x)`$. | [Dynamic semantics 2.5](dynamic-semantics.md#25-temperature) |
| `cr_round_decided` | If both ends of an enclosure round to one value, that value is $`\mathrm{RN}_p(f(x))`$. | [Dynamic semantics 3.3](dynamic-semantics.md#33-the-ziv-loop) |
| `selection_reuse` | The order of eligible records instantiates `Similarity.Selection`. | [Integration 2](integration.md#2-score-queries-tl-0) |
| `score_record_local_elig` | A record-local score meets the section hypotheses of `VectorDbDiscipline.v` on `elig` and `esort_insert`, so `vdb_no_pending_match` holds. | [Static semantics 9](static-semantics.md#9-record-locality-of-scores) |

### 1.3 TensorGSLT.v

The module `formal/rocq/reified_rspaces/theories/TensorGSLT.v` models the surface, the core and the normalizer.

| Theorem | Statement | Claim |
|---|---|---|
| The syntax | Inductive types for the surface terms and the core terms | [Syntax 2](syntax.md#2-the-grammar) |
| `sugar_sound_k`, one for each of S1 to S21 | Each rewrite keeps the denotation. The positional forms are checked against an independent definition, the rules of numpy. | [GSLT 3](gslt.md#3-the-sugar-and-its-rewrites) |
| `sugar_trs_orthogonal`, `elab_terminates` | The sugar rules are left-linear and do not overlap, and ELABORATE terminates. | [GSLT 5.2](gslt.md#52-why-the-normal-form-is-unique) |
| `nf_sound`, `nf_idempotent`, `nf_complete_ETL` | The NF keeps the denotation, normalizes to itself, and is equal for terms that are equal under $`E_{TL}`$. | [GSLT 5.2](gslt.md#52-why-the-normal-form-is-unique) |
| `typing_progress_preservation` | A term that passes the checks has no shape error at run time. | [Static semantics 2](static-semantics.md#2-shape-inference-and-typing-rules) |
| `demand_bound_sound`, `sufficient_budget_never_rejects` | The demand bound covers every reservation, so a budget of at least $`B`$ never rejects. | [Static semantics 10](static-semantics.md#10-the-static-demand-bound) |
| `reflect_decode_roundtrip` | The decoder inverts the reflection that [GSLT 6](gslt.md#6-the-reflection-of-tl-terms) specifies. A differential test checks the reflector of F1r3lang against that specification. | [GSLT 6](gslt.md#6-the-reflection-of-tl-terms) |

### 1.4 TLA+

TL adds no protocol, so it adds no TLA+ module.
`formal/tlaplus/reified_rspaces/VectorDb.tla` gains a score abstraction: a total preorder that depends on each record alone.
A new safe configuration checks every invariant of the module with that abstraction.
A new Unsafe flag `NonLocalScore` lets the score of a record depend on another record, and its configuration must refute `QuiescentNoPendingMatch`.

## 2. Formal verification and negative controls

### 2.1 The legs

| Leg | Location | What it checks |
|---|---|---|
| Rocq | `scripts/check-reified-rspaces-proofs.sh` | Both modules compile, `Print Assumptions` shows a closed context, and the headline count rises by the theorems of section 1. |
| TLA+ | `scripts/check-reified-rspaces-tla-invariants.sh` | TLC passes the safe configuration, and the Unsafe configuration fails with `QuiescentNoPendingMatch` only. |
| Z3 | `formal/z3/reified_rspaces/` | `tl_broadcast_numpy.py` (the rule of numpy against elaboration, up to rank 4), `tl_matmul_cases.py`, `tl_index_interval.py` (sound enclosures of affine index expressions with `/` and `%`), `tl_cmp_logic_sugar.py` (S3 to S6 over $`\{0, 1\}`$ and $`\mathbb{Q}`$), `tl_maj.py` and `tl_rn_ties_even.py` |
| Sage | `formal/sage/reified_rspaces/` | `tl_correct_rounding.sage`: reference values of every CR function at $`p \in \{8, 24, 53, 64, 113, 256\}`$, with the hard cases of Lefèvre and Muller ([LM01](#ref-lm01)). `tl_paper_programs.sage`: the exact outputs of [syntax 4](syntax.md#4-worked-examples). `tl_temperature.sage`: the threshold property and the limit of `softmax` as $`T \to 0^{+}`$. A differential test against numpy on inputs where numpy is exact. |
| Rewriting | `formal/rewriting/tensor_logic/tl_sugar.trs` | TTT2 proves the termination, and CSI proves the confluence, of the local sugar rules S1 to S8. |

A missing tool fails its leg.
The rewriting leg of the cost-accounting work is fail-soft (`scripts/check-cost-accounted-rho-rewriting.sh`, lines 22 to 25), and the TL leg must not copy that behavior.

### 2.2 The negative controls

Each mutant below must break the theorem or the test in the second column.

| Mutant | Must break |
|---|---|
| The scope rule aggregates at the outermost node | `scope_rule_paper_examples`, `tl_paper_programs.sage` |
| The join is strict in $`\bot`$ | `zero_absorbs_bot`, `prop_tl_zero_absorbs_undefined` |
| The rounding breaks ties upward | `tl_rn_ties_even.py`, `prop_tl_cr_matches_sage` |
| Broadcasting ignores the size-1 rule | `tl_broadcast_numpy.py`, `prop_tl_broadcast_matches_numpy` |
| The tie key is dropped | `selection_reuse`, `tl_score_` |
| A negation that is not stratified is accepted | `stratified_perfect_model`, `tl_datalog_` |
| The guard check is skipped | `tl_score_` with a `where` guard |
| A sum accumulates in `f64` | `prop_tl_eval_deterministic` on $`[10^{16}, 1, -10^{16}]`$ |
| A CR function is wrong by one unit in the last place | `prop_tl_cr_matches_sage` |
| A score reads another record | `score_record_local_elig`, the Unsafe configuration of `VectorDb.tla`, `prop_tl_score_record_local` |
| `log 0` gives 0 | `builtin_domains`, `prop_tl_partial_functions_give_bottom` |
| The fold of a rule also folds from the left | CSI on `tl_sugar_overlap.trs`, which must not prove confluence |

## 3. Invariants as property tests

Every invariant has a property test, and `formal/reified-rspaces-invariant-map.json` maps each one to its model.
The checker `scripts/verify-reified/check-invariant-map.py` fails on an invariant without a property test.

| Property test | Invariant | Model |
|---|---|---|
| `prop_tl_eval_deterministic` | Evaluation is a function of the term and the inputs | `denote_functional` |
| `prop_tl_partial_functions_give_bottom` | Each function gives $`\bot`$ exactly outside its domain | `builtin_domains` |
| `prop_tl_ac_invariant` | Permutations of joins, sums and statements keep the value | `join_perm_invariant`, `sum_perm_invariant`, `program_perm_invariant` |
| `prop_tl_alpha_invariant` | Renamings of bound indices keep the value | `alpha_index_invariant` |
| `prop_tl_nf_idempotent` | The NF of an NF is itself | `nf_idempotent` |
| `prop_tl_nf_preserves_denotation` | The NF keeps the value | `nf_sound` |
| `prop_tl_sugar_matches_core` | Each sugar form has the value of its rewrite | `sugar_sound_k` |
| `prop_tl_installed_rewrites_agree` | F1r3lang with the installed rewrites and the native normalizer agree on S1 and S2 (open question K15) | `sugar_trs_orthogonal` |
| `prop_tl_broadcast_matches_numpy` | Elaboration follows the broadcasting of numpy | `tl_broadcast_numpy.py` |
| `prop_tl_zero_absorbs_undefined` | Dense and sparse evaluation agree | `zero_absorbs_bot` |
| `prop_tl_datalog_seminaive_eq_naive` | Semi-naive and naive iteration reach one fixpoint | `seminaive_least_fixpoint` |
| `prop_tl_cr_monotone` | A CR function keeps the order of a monotone function | `cr_round_decided` |
| `prop_tl_cr_matches_sage` | CR values equal the reference values | `tl_correct_rounding.sage` |
| `prop_tl_demand_bound_sound` | The reservations stay within the demand bound | `demand_bound_sound` |
| `prop_tl_play_eq_replay_reservations` | Native play and native replay reserve the same host work | `denote_functional`, `demand_bound_sound` |
| `prop_tl_score_record_local` | A score depends on its own record only | `score_record_local_elig` |
| `prop_tl_unicode_nfc_idempotent` | The identifier canon is idempotent | E10 of `nf_complete_ETL` |
| `prop_tl_confusable_rejected` | A confusable identifier fails | The rules of [syntax 1.3](syntax.md#13-identifiers) |
| `prop_tl_identity_bytes_roundtrip` | The identity bytes decode to the identity | The identity encoder of [decision 74](../decision-register.md#decision-74) |

## 4. A complete implementation

- The FIPS conformance matrix of phase P10 covers lines 255 to 261 of the Reifying RSpaces FIPS: the VectorDB rules and the tensor-logic rule.
- No code is ported from the legacy `tensor/` module ([design rationale 1](design-rationale.md#1-the-legacy-support)), and no source holds an incomplete marker.
- A test recomputes the fingerprint $`\mathcal{F}_{TL}`$ from the module source and compares it with the pinned value.
- A test reaches every diagnostic code of [static semantics 12](static-semantics.md#12-diagnostics).
- A test covers every row of the signature table ([static semantics 2.3](static-semantics.md#23-calls)) and of the function table ([dynamic semantics 2.6](dynamic-semantics.md#26-built-in-functions)).

## 5. Thorough tests

- **Suites.** `tl_lex_`, `tl_parse_`, `tl_resolve_`, `tl_elab_`, `tl_nf_`, `tl_check_`, `tl_eval_`, `tl_cr_`, `tl_datalog_`, `tl_score_`, `tl_vector_eval_`, `tl_theory_` and `tl_unicode_`, in new files `rholang/tests/reified_tl_*.rs`.
- **One reading.** `tl_parse_` parses every example of [syntax 4](syntax.md#4-worked-examples) and requires exactly one reading.
- **Native certification.** Native play and native replay at the host-work limits of protocol 6 ([decision 69](../decision-register.md#decision-69), [decision 78](../decision-register.md#decision-78), [decision 79](../decision-register.md#decision-79)).
- **Merges.** Score queries on concurrent branches, with the cases of P6.
- **Griefing.** The crafted embedding of finding F4 ([README](README.md#4-the-findings-of-the-design-pass)).

## 6. Documentation

- This directory and its diagrams 35 to 54.
- The specification gains the TL keys in section 3.9 and the TL methods in section 9. The syntax contract gains the TL keys and the threshold rule.
- The glossary gains the TL terms, and the decision register records the approval of the grammar (K0) and the answers to K1 to K16.
- Chapter 21 of the Rholang guide gains a TL section.
- The document lint passes on every document, and every diagram renders.

## References

<a id="ref-abw88"></a>[ABW88] K. R. Apt, H. A. Blair, and A. Walker, "Towards a theory of declarative knowledge," in *Foundations of Deductive Databases and Logic Programming*, Morgan Kaufmann, 1988, pp. 89–148. [doi:10.1016/B978-0-934613-40-8.50006-3](https://doi.org/10.1016/B978-0-934613-40-8.50006-3)

<a id="ref-dom25"></a>[Dom25] P. Domingos, "Tensor Logic: The Language of AI," arXiv:2510.12269v3, 2025. [doi:10.48550/arXiv.2510.12269](https://doi.org/10.48550/arXiv.2510.12269)

<a id="ref-lm01"></a>[LM01] V. Lefèvre and J.-M. Muller, "Worst cases for correct rounding of the elementary functions in double precision," in *Proceedings of the 15th IEEE Symposium on Computer Arithmetic (ARITH-15)*, 2001, pp. 111–118. [doi:10.1109/ARITH.2001.930110](https://doi.org/10.1109/ARITH.2001.930110)
