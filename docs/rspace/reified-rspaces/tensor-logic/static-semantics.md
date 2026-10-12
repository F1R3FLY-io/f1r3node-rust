# TensorLogic static semantics

This document specifies the checks that a TensorLogic (TL) term passes before evaluation.
The checks infer shapes and kinds, place every implicit projection, reject unsafe or unbounded terms, and compute a bound on the work of evaluation.
Every check fails closed with a typed code ([section 12](#12-diagnostics)).
The terms are defined in [concepts](concepts.md), and the grammar is in [syntax](syntax.md).

Figure TL5 shows the order of the checks.

![Activity diagram of the static checks of TensorLogic, in four partitions. RESOLVE checks the names and the call signatures, with exits for UnknownTensor and ArityMismatch. ELABORATE checks the broadcasting and the matrix products of the positional sugar, with exits for BroadcastMismatch and MatmulMismatch. SCOPE places every implicit projection by the scope rule. CHECK infers the shapes and the kinds by unification, with exits for ShapeMismatch and IndexUnbounded. It then checks the constant arguments, the safety of rules, accumulation, the recursion classes and, for a score, record-locality, each with its exit. Last it computes the demand bound and checks the static limits, with an exit for TensorTooLarge, and the term goes to evaluation.](../diagrams/40-tl-static-checks.svg)

*Figure TL5. Source: [diagrams/40-tl-static-checks.puml](../diagrams/40-tl-static-checks.puml), rendered by [diagrams/render.sh](../diagrams/render.sh).*

## Contents

1. [Sorts](#1-sorts)
2. [Shape inference and typing rules](#2-shape-inference-and-typing-rules)
3. [The scope rule](#3-the-scope-rule)
4. [Broadcasting and the positional sugar](#4-broadcasting-and-the-positional-sugar)
5. [Range restriction and safety](#5-range-restriction-and-safety)
6. [Composition, accumulation and inputs](#6-composition-accumulation-and-inputs)
7. [Recursion classes and stratification](#7-recursion-classes-and-stratification)
8. [Let](#8-let)
9. [Record-locality of scores](#9-record-locality-of-scores)
10. [The static demand bound](#10-the-static-demand-bound)
11. [Static limits](#11-static-limits)
12. [Diagnostics](#12-diagnostics)

The [references](#references) follow the last section.

## 1. Sorts

TL has five kinds of static information.

| Sort | Values | Note |
|---|---|---|
| Kind | `Num` and `Bool` | $`\mathsf{Bool} \le \mathsf{Num}`$: a Boolean tensor can enter arithmetic. |
| Dimension | `Pos(n)` or `Sym`, each hard or soft | A hard dimension comes from data or a declaration. A soft dimension is an inferred bounding box. |
| Shape | A list of dimensions | The rank is at most 8. |
| Function type | $`\mathsf{Num}^{n} \to \mathsf{Num}`$ | First order only. A lambda is sugar for a named function. |
| Index expression | Sort `Pos` or `Sym` | A `Pos` index is an affine term with constant coefficients, with `/` and `%` by positive constants. A `Sym` index is a variable or an atom. |

The signature $`\Sigma`$ maps each tensor name to its kind, its shape and its provenance.
The provenance is one of input, defined, private (bound by `new`) and reserved (bound by an agent).

## 2. Shape inference and typing rules

### 2.1 Inference

The checker infers dimensions and their kinds by unification.

1. The domain of an index variable is the common size of its hard plain occurrences. Two different sizes fail with `ShapeMismatch`, and the message names the index, the sizes and the tensors.
2. An index with no hard occurrence takes the largest soft size, with zero padding as in the paper ([Dom25](#ref-dom25), page 7).
3. An index with no occurrence at all has no finite domain and fails with `IndexUnbounded`.
4. The shape of a defined tensor is the bounding box of the images of its left side. Sound interval arithmetic over the index expressions computes it. A possible negative image fails with `LhsNegative`.
5. Inside a recursive component, a domain must not come from tensors of the same component (`IndexUnbounded`). So the fixpoint of shapes ends.

### 2.2 Typing rules

The rules use the judgment $`\Sigma; \Delta \vdash e : \kappa`$: under the signature $`\Sigma`$ and the index context $`\Delta`$, the expression $`e`$ has the kind $`\kappa`$.
The index context maps each index variable to its dimension.

```math
\frac{\Sigma(A) = (\kappa, [d_1, \ldots, d_r]) \qquad \Delta \vdash \iota_k : d_k \ \ (1 \le k \le r)}
     {\Sigma; \Delta \vdash A[\iota_1, \ldots, \iota_r] : \kappa}\ \ (\mathsf{T\text{-}Acc})
\qquad
\frac{\Sigma(R) = (\mathsf{Bool}, [d_1, \ldots, d_r]) \qquad \Delta \vdash \iota_k : d_k \ \ (1 \le k \le r)}
     {\Sigma; \Delta \vdash R(\iota_1, \ldots, \iota_r) : \mathsf{Bool}}\ \ (\mathsf{T\text{-}BAcc})
```

```math
\frac{\Sigma; \Delta \vdash e_1 : \kappa_1 \qquad \Sigma; \Delta \vdash e_2 : \kappa_2}
     {\Sigma; \Delta \vdash e_1\, e_2 : \kappa_1 \sqcup \kappa_2}\ \ (\mathsf{T\text{-}Join})
\qquad
\frac{\Sigma; \Delta, J \vdash e : \kappa \qquad J \cap \mathrm{fv}(\mathrm{outside}) = \emptyset}
     {\Sigma; \Delta \vdash \mathrm{op}[J](e) : \kappa'}\ \ (\mathsf{T\text{-}Proj})
```

In rule T-Proj, $`\kappa'`$ is `Bool` for `max`, `min` and `maj` over a Boolean body, and `Num` otherwise.
The side condition says that the bound indices $`J`$ do not occur outside the projection.

```math
\frac{\Sigma; \Delta \vdash \lambda : \mathit{LHS} \qquad \Sigma; \Delta \vdash R : \kappa \qquad \mathrm{fv}(R) \subseteq \mathrm{vars}(\lambda)}
     {\Sigma \vdash T[\lambda] \oplus{=}\, R \ \ \mathsf{ok}}\ \ (\mathsf{T\text{-}Eq})
```

Rule T-Eq applies after the scope rule of [section 3](#3-the-scope-rule) makes every projection explicit.
Then every free index of the right side occurs on the left side.
A Boolean left side forces the result kind `Bool`, through the step function.

### 2.3 Calls

Each call has a fixed signature, which RESOLVE checks before elaboration.
[Dynamic semantics 2.6](dynamic-semantics.md#26-built-in-functions) gives the value of each built-in function.

| Call | Positional arguments | Keyword arguments |
|---|---|---|
| `step`, `relu`, `abs`, `sign`, `hardsig`, `exp`, `log`, `sqrt`, `rsqrt`, `tanh`, `sin`, `cos` | 1 | None |
| `sig`, `sigmoid` | 1, or 2 with the temperature | None |
| `min` | 2 | None |
| `max` | 2, or 1 for a reduction | `axis`, with 1 positional argument only |
| `sum` | 1 | `axis` |
| `clamp` | 3: the value, the lower bound and the upper bound | None |
| `softmax`, `lnorm` | 1, or 2 with the temperature or $`\varepsilon`$ | `axis`, in the positional style only |
| `softmax[k]`, `lnorm[k]` | 1, or 2 with the temperature or $`\varepsilon`$ | None |
| `concat` | 1 | None |
| `transpose` | 1, or 2 with a permutation list | None |
| `einsum` | A string, then one operand for each operand term of the string | None |
| `op[J]` (an explicit projection) | 1 | None |
| A user function or operator | The number of its parameters | None |

- A wrong number of positional arguments, an unknown or repeated keyword, or a positional argument after a keyword fails with `ArityMismatch`.
- A temperature, an $`\varepsilon`$, an `axis`, a permutation, an `einsum` string, and the bounds and step of a slice must be constant. A constant holds only numerals, strings, `let` constants and size variables. An index or a tensor in such an argument fails with `NotConstant`.
- A negative temperature fails with `TemperatureNegative`.
- `step` gives the kind `Bool`, and `min` and `max` give `Bool` when both arguments are `Bool`. Every other call gives `Num`.

## 3. The scope rule

### 3.1 The problem

An equation sums over every index that its left side does not name.
The paper places that sum at different depths in different examples ([Dom25](#ref-dom25)).
In `Y = step(W[i] X[i])`, the sum over `i` is inside `step` (page 6).
In the Gaussian kernel, the sum over `j` is outside the square, but in the polynomial kernel it is inside the power (page 11).
The scope rule reproduces every example of the paper with one deterministic rule, which the approved P6 study defined.

### 3.2 The rule

1. **Candidates.** The candidate scope nodes are every product node (`Term`) and every argument of a call. Calls include built-in and user functions, the reciprocal of `/`, normalizations and explicit projections. The root of the right side counts as a call argument. The base of `**` is not a candidate.
2. **Scope.** Take an index $`j`$ that the left side does not name and that no explicit projection binds. Then $`\mathrm{scope}(j)`$ is the deepest candidate that contains every occurrence of $`j`$.
3. **Spine.** The spine is a path from the root of the right side. The path extends from its last node to the one non-constant child of that node when the node has one of these forms. The node where the path stops is the last node of the spine.
   - A sum with one term
   - A product with one non-constant factor
   - A univariate elementwise call with one non-constant argument
   - A power with a constant exponent
   - Parentheses
4. **Operator.** The checker aggregates $`j`$ at $`\mathrm{scope}(j)`$ with the operator $`\oplus`$ of the equation when that node is on the spine, and with `sum` otherwise. For $`\oplus`$ = sum, this is the rule of the P6 study.

The spine exists because an equation such as `Y max= relu(A[j])` means $`\mathrm{relu}(\max_j A_j)`$, the reading $`\varphi(\pi(\bowtie))`$ of the paper (page 6).
Off the spine, a projection inside a larger expression is a contraction, which is always a sum.

### 3.3 The rule on the examples of the paper

| Equation | Scope of the free index | Meaning |
|---|---|---|
| `Y = step(W[i] X[i])` | The product inside `step` | $`H(\sum_i W_i X_i)`$ |
| `Comp[p, p'.] = softmax(Query[p, dk] Key[p', dk] / sqrt(Dk))` | The product inside `softmax` | The sum over `dk` comes before the normalization (p. 10). |
| `K[q, i] = exp(-(Xq[q, j] - Xs[i, j]) ** 2 / Var)` | The product outside the square | $`\exp(-\sum_j (Xq_{qj} - Xs_{ij})^2 / \mathit{Var})`$ |
| `K[q, i] = (Xq[q, j] Xs[i, j]) ** N` | The product inside the power | $`(\sum_j Xq_{qj} Xs_{ij})^N`$ |
| `Y max= relu(A[j])` | The argument of `relu`, on the spine | $`\mathrm{relu}(\max_j A_j)`$ |
| `Pooled[x / 2, y / 2] max= F[x, y]` | The left side groups the blocks | The maximum of each two-by-two block (p. 9) |

Figure TL6 draws the scope rule on the Gaussian kernel.

![Tree diagram of the scope rule on the Gaussian kernel K of q and i equals exp of minus the square of the difference of Xq of q and j and Xs of i and j, divided by Var. The root is the call of exp, a candidate, because the root of the right side counts as a call argument. Below it is its argument, the product of the negated square and the reciprocal of Var, also a candidate. Below that is the power with the constant exponent 2, whose base is not a candidate. The base is the difference of the two accesses, which hold the two occurrences of j. The deepest candidate that contains both occurrences of j is the product node, so the sum over j sits there, outside the square. The spine extends from the call of exp to the product node and stops there, because the product has two non-constant factors. So the operator of the equation, sum, aggregates j at the product node.](../diagrams/41-tl-scope-rule.svg)

*Figure TL6. Source: [diagrams/41-tl-scope-rule.puml](../diagrams/41-tl-scope-rule.puml), rendered by [diagrams/render.sh](../diagrams/render.sh).*

## 4. Broadcasting and the positional sugar

### 4.1 Broadcasting

The positional sugar follows the broadcasting rules of numpy exactly ([HMW20](#ref-hmw20)).

1. Align the two shapes from the right, and pad the shorter shape on the left with dimensions of size 1.
2. For each axis $`k`$: if $`a_k = b_k`$, the result is $`a_k`$. If $`a_k = 1`$, the result is $`b_k`$. If $`b_k = 1`$, the result is $`a_k`$. Otherwise the check fails with `BroadcastMismatch`, and the message names both shapes and the negative axis, as numpy does.
3. A dimension of size 0 broadcasts against size 1 to size 0.
4. Elaboration gives each result axis a fresh index. An operand axis of size 1 against a result size other than 1 reads its only element.

### 4.2 Matrix product

The operator `@` follows `numpy.matmul`.

- A 1-D operand by a 1-D operand is the inner product, and 2-D by 2-D is the matrix product.
- A 1-D first operand gets a leading axis of size 1, which the result drops again. A 1-D second operand gets a trailing axis of size 1.
- For more than two dimensions, the last two axes multiply and the leading axes broadcast.
- A scalar operand or a mismatched inner size fails with `MatmulMismatch`.

Figure TL7 shows the decision for the positional operators.

![Activity diagram of the elaboration of the positional operators. For an elementwise operator, the shapes are aligned from the right and padded with ones, each axis is compared, equal sizes or a size of one give the result size, and any other pair exits with BroadcastMismatch, which names the shapes and the axis. For the matrix product, a scalar operand exits with MatmulMismatch. Two one-dimensional operands give the inner product. Otherwise a one-dimensional first operand gets a leading axis of size one and a one-dimensional second operand gets a trailing axis of size one. The last two axes multiply, and a mismatch of the inner size exits with MatmulMismatch. The leading axes broadcast by the same rule as the elementwise operators. The result is an index-form term with a fresh index for each result axis and an explicit sum over the inner axis.](../diagrams/42-tl-broadcast-matmul.svg)

*Figure TL7. Source: [diagrams/42-tl-broadcast-matmul.puml](../diagrams/42-tl-broadcast-matmul.puml), rendered by [diagrams/render.sh](../diagrams/render.sh).*

### 4.3 Reductions, axes and einsum

- `.T` reverses the axes, and `transpose(A, [perm])` permutes them.
- `sum` and `max` with `axis=k` or `axis=[…]` accept negative axes. Without `axis`, they reduce every axis.
- `softmax(A, axis=k)` requires `axis` for a rank of 2 or more (`AxisRequired`).
- `einsum` takes labels `[a-zA-Z]`. The explicit mode uses `->`. The implicit mode outputs the labels that appear once, in alphabetical order, as numpy does.
- In `einsum`, a label repeated in one operand takes the diagonal, and a label shared between operands must have one size. An ellipsis fails with `EinsumEllipsis`.
- A slice has constant bounds and the negative wrap of numpy. An integer index lowers the rank by one.

### 4.4 Mixing the styles

The form `(E)[i, j]` names the positional axes of `E` with indices.
A positional right side assigned to an indexed left side names its axes by the left indices.
An index-style operand next to an unindexed positional operand fails with `MixedStyle`, with the hint "index it: (E)[i]".

## 5. Range restriction and safety

- Every index of a left side needs a finite domain: from the right side, from a declaration, or from a size pattern.
- Boolean rules must be safe in the sense of Datalog ([CGT89](#ref-cgt89)). Every variable of a head, of a negated literal or of a comparison occurs in a positive body atom (`UnsafeVariable`).
- A fact must be ground (`NonGroundFact`).
- An index expression on the right side reads 0 outside the shape. A negative constant index in index style fails with `NegativeIndexConstant`.

## 6. Composition, accumulation and inputs

- All statements that define one tensor agree on its kind, its rank and its operator $`\oplus`$ (`AccumulationMismatch`).
- An input cannot also be defined (`InputRedefined`).
- A normalized left side cannot accumulate.
- A private tensor, bound by `new`, cannot be queried (`PrivateQuery`).

## 7. Recursion classes and stratification

The dependency graph has an edge from each tensor of a right side to the tensor of the left side.
An edge is non-monotone when it passes through `not`, `1 - x`, a subtraction, a reciprocal or a non-monotone function, and monotone otherwise.
Each strongly connected component of the graph belongs to one class.

| Class | Condition | Allowed in |
|---|---|---|
| R0 | No cycle | Every category |
| R1, index-guarded numeric recursion | Every recursive read is at a guard index $`g`$ strictly below the write: the left side uses $`g + c`$, the right side reads $`g + c'`$, and $`c' < c`$. The domain of $`g`$ is hard and positional. | Programs only |
| R2, Boolean monotone recursion | Every edge of the component is monotone, as in Datalog | Programs only |

- A component outside these classes fails with `UnguardedRecursion` or `NotStratified`.
- Negation must be stratified: no cycle passes through a negated edge ([ABW88](#ref-abw88)).
- A virtual index `*t` may be read only at `*t + c` inside its own recurrence (`VirtualIndexMisuse`).
- A score or a check term must not be recursive (`ScoreRecursive`), as [decision 53](../decision-register.md#decision-53) requires. Finding F7 explains why programs may still recurse ([README](README.md#4-the-findings-of-the-design-pass)).

## 8. Let

- A `let` is lexically scoped and has no recursive form. The name of a definition is not in scope in its own body, so recursion is impossible by construction. A reference to the name inside its body fails with `UnknownFunction`.
- Parameters are first order.
- Inlining is capture-avoiding. The checker first places the projections of each call argument, and then it substitutes the argument with its bound indices renamed apart.
- A static limit bounds the size of the expansion (`InlineTooLarge`).

## 9. Record-locality of scores

A score must depend on its own record only.
In the normal form of a score, three conditions hold.

1. The record index `x` occurs only in the implicit left side `score[x]` and as the first index of `Emb[x, …]`.
2. No projection and no normalization binds `x`.
3. No index arithmetic uses `x`, and no expression uses it as a value.

A violation fails with `ScoreNotRecordLocal`.

**Lemma (record-locality).** Let a score satisfy the three conditions.
Then its value for a record $`r`$ depends only on the embedding row $`\mathrm{Emb}[r, \cdot]`$, the query vector $`q`$ and the `tensors` param.

*Proof.* By induction on the normal form.
An access `Emb[x, e]` reads only the row $`r`$, because condition 1 fixes its first index to `x`, and $`\rho(x) = r`$.
The other tensors in a score are `q` and the tensors of the param.
None of them has `x` as an index, by condition 1.

A projection or a normalization enumerates only its bound indices, and by condition 2 these never include `x`.
Index arithmetic and value uses of `x` are absent by condition 3.
So every subterm evaluates under valuations that agree on `x = r`, and it reads only the row $`r`$ and the record-independent tensors. ∎

This lemma is the premise of two section declarations of the VectorDB model: the eligibility function `elig` and the order hypothesis `esort_insert`.
Lines 228 and 232 of `VectorDbDiscipline.v` declare them.
So the theorem `vdb_no_pending_match` (`VectorDbDiscipline.v:662`) holds for TL scores too ([integration 2](integration.md#2-score-queries-tl-0)).

## 10. The static demand bound

The demand bound $`B(P, \Gamma, p)`$ is a triple of operations, scanned bytes and backing bytes.
It is a function of the normal form $`P`$, the input shapes $`\Gamma`$ (with the bit sizes of the input numbers) and the precision $`p`$.
The checker computes it in time linear in the size of the normal form:

```math
B = \sum_{s \in \mathrm{slice}(P)} \mathrm{rounds}(s) \cdot \lvert D_{L_s} \rvert \cdot c(R_s)
```

```math
c(\mathrm{op}[J](e)) = \lvert D_J \rvert \,\big(c(e) + c_{\oplus}(\beta)\big), \qquad
c(e_1\, e_2) = c(e_1) + c(e_2) + c_{\times}(\beta_1, \beta_2), \qquad
c(f(e)) = c(e) + c_f(p, \beta)
```

| Symbol | Meaning |
|---|---|
| $`\mathrm{slice}(P)`$ | The statements that the queries need |
| $`L_s`$, $`R_s`$ | The left side and the right side of the statement $`s`$ |
| $`\mathrm{rounds}(s)`$ | 1 for class R0, $`\lvert D_g \rvert`$ for class R1, and at most the number of possible derived tuples for class R2 |
| $`\beta`$ | Bounds on the bit sizes of values, propagated through the operations |
| $`c_{\oplus}`$, $`c_{\times}`$ | The unit costs of an aggregation step and of a multiplication |
| $`c_f`$ | For a correctly rounded function, one Ziv step for each width of its schedule ([dynamic semantics 3](dynamic-semantics.md#3-correctly-rounded-functions)) |

The unit costs are those of the tensor module of P6 (`rholang/src/rust/interpreter/tensor/costs.rs:8-23`).
The bound is advisory: evaluation reserves its work step by step, as [decision 40](../decision-register.md#decision-40) requires.
A budget of at least $`B`$ never ends in `HostWorkRejected` ([verification plan](verification-plan.md#1-formal-models)).

## 11. Static limits

Static limits bound the size of terms and states, not the size of scans, which [decision 40](../decision-register.md#decision-40) allows.

| Limit | Value |
|---|---|
| Nodes of a normal form | 4,096 for a program, and 512 for a score or a check |
| Rank of a tensor | 8 |
| Index variables | 16 for each equation, and 8 bound by one projection |
| Elements of one input | 65,536 |
| Precision $`p`$ | From 8 to 256 bits |

A violation fails with `TensorTooLarge` or `PrecisionOutOfRange`.

## 12. Diagnostics

Every check fails closed.
A diagnostic text is deterministic and holds at most 512 characters.
The language installer of F1r3lang bounds its public errors the same way (`MAX_PUBLIC_ERROR_CHARS` in `rholang-runtime/src/language_install.rs`, mettail-rust at `c68bebf5`).
TL terms carry no source spans, so a diagnostic names tensors, indices, shapes and axes.
F1r3lang reports syntax errors with spans when it constructs an FLT.
A hook for static checks at construction is missing facility MF5 ([design rationale 4](design-rationale.md#4-missing-f1r3lang-facilities)).

| Phase | Codes |
|---|---|
| Decode | `NotATensorTerm`, `UnknownTensorLanguage` |
| Lexical and Unicode | `NotNfc` (a warning: the term is normalized), `ConfusableIdentifier`, `MixedScriptIdentifier`, `NonAsciiOperator`, `LiteralKind`, `LiteralRange` |
| Resolution | `UnknownTensor` (with a suggestion), `UnknownFunction`, `AmbiguousName`, `ReservedName`, `BinderClash`, `OperatorMix`, `ArityMismatch` |
| Elaboration | `MixedStyle`, `BroadcastMismatch`, `MatmulMismatch`, `AxisOutOfRange`, `AxisRequired`, `EinsumSpec`, `EinsumEllipsis`, `SliceOutOfRange`, `RankUnknown` |
| Checking | `ShapeMismatch`, `IndexUnbounded`, `LhsNegative`, `NegativeIndexConstant`, `NotBoolean`, `UnsafeVariable`, `NonGroundFact`, `AccumulationMismatch`, `InputRedefined`, `PrivateQuery`, `NotStratified`, `UnguardedRecursion`, `VirtualIndexMisuse`, `ScoreNotRecordLocal`, `ScoreRecursive`, `NoQuery`, `InlineTooLarge`, `TensorTooLarge`, `PrecisionOutOfRange`, `TemperatureNegative`, `NotConstant` |
| Evaluation | `Undefined` (an output element is $`\bot`$), `PrecisionExhausted`, `NonFiniteValue`, `NotANumber`, `MalformedNumber`, `DimensionMismatch`, `ValueOutOfRange`, `HostWorkRejected` |

Figure TL8 groups the codes by phase.

![Mind map of the TensorLogic diagnostic codes, grouped by the phase that raises them: decode, lexical and Unicode, resolution, elaboration, checking and evaluation. Each branch lists the codes of its phase, from NotATensorTerm and UnknownTensorLanguage in decode to Undefined, PrecisionExhausted and HostWorkRejected in evaluation.](../diagrams/53-tl-error-codes.svg)

*Figure TL8. Source: [diagrams/53-tl-error-codes.puml](../diagrams/53-tl-error-codes.puml), rendered by [diagrams/render.sh](../diagrams/render.sh).*

## References

<a id="ref-abw88"></a>[ABW88] K. R. Apt, H. A. Blair, and A. Walker, "Towards a theory of declarative knowledge," in *Foundations of Deductive Databases and Logic Programming*, Morgan Kaufmann, 1988, pp. 89–148. [doi:10.1016/B978-0-934613-40-8.50006-3](https://doi.org/10.1016/B978-0-934613-40-8.50006-3)

<a id="ref-cgt89"></a>[CGT89] S. Ceri, G. Gottlob, and L. Tanca, "What you always wanted to know about Datalog (and never dared to ask)," *IEEE Transactions on Knowledge and Data Engineering*, vol. 1, no. 1, pp. 146–166, 1989. [doi:10.1109/69.43410](https://doi.org/10.1109/69.43410)

<a id="ref-dom25"></a>[Dom25] P. Domingos, "Tensor Logic: The Language of AI," arXiv:2510.12269v3, 2025. [doi:10.48550/arXiv.2510.12269](https://doi.org/10.48550/arXiv.2510.12269)

<a id="ref-hmw20"></a>[HMW20] C. R. Harris et al., "Array programming with NumPy," *Nature*, vol. 585, pp. 357–362, 2020. [doi:10.1038/s41586-020-2649-2](https://doi.org/10.1038/s41586-020-2649-2)
