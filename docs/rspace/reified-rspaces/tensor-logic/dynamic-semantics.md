# TensorLogic dynamic semantics

This document gives the meaning of every core construct of TensorLogic (TL), the exact arithmetic, the correctly rounded functions and the evaluation algorithm.
The meaning applies to the normal form (NF) that the normalizer computes ([GSLT 5](gslt.md#5-normalization)), after the static checks of [static semantics](static-semantics.md).
The terms are defined in [concepts](concepts.md).

## Contents

1. [Values and operations](#1-values-and-operations)
2. [Denotation](#2-denotation)
3. [Correctly rounded functions](#3-correctly-rounded-functions)
4. [Exact arithmetic](#4-exact-arithmetic)
5. [The evaluation algorithm](#5-the-evaluation-algorithm)
6. [Demand](#6-demand)
7. [Determinism, termination and resource bounds](#7-determinism-termination-and-resource-bounds)
8. [Consensus state](#8-consensus-state)

The [references](#references) follow the last section.

## 1. Values and operations

The value domain is $`\mathbb{Q}_{\bot} = \mathbb{Q} \cup \{\bot\}`$: the rationals and the undefined value $`\bot`$.
Division by zero, and an argument outside the domain of a function, are the only sources of $`\bot`$ in the core ([section 2.6](#26-built-in-functions)).

| Operation | Definition | Reason |
|---|---|---|
| Join $`a \otimes b`$ | 0 if $`a = 0`$ or $`b = 0`$, even when the other value is $`\bot`$. Otherwise $`ab`$ if both are rational, and $`\bot`$ otherwise. | Zero absorbs $`\bot`$, so a dense evaluation and a sparse evaluation agree. It also supports the guards of the paper: a conditional is a join with the condition ([Dom25](#ref-dom25), p. 10). |
| Sum $`a + b`$ | Strict in $`\bot`$ | A sum over an undefined element is undefined. |
| Reciprocal $`\mathrm{recip}(a)`$ | $`1/a`$ for $`a \ne 0`$, and $`\bot`$ otherwise | `/` is a join with the reciprocal. |
| Function $`f(a)`$ | Strict in $`\bot`$, and $`\bot`$ outside the domain of $`f`$ | No built-in function invents a value. |

## 2. Denotation

### 2.1 Expressions

The denotation $`[\![e]\!]_\rho`$ of an expression depends on a valuation $`\rho`$ of its index variables.
$`\Theta(A)`$ is the value of the tensor $`A`$.
$`D_J`$ is the domain of the bound indices $`J`$, and $`\rho[\sigma]`$ extends $`\rho`$ with the values $`\sigma`$ of $`J`$.

```math
\begin{aligned}
[\![A[\iota]]\!]_\rho &= \begin{cases} \Theta(A)\big([\![\iota]\!]_\rho\big) & \text{inside the shape of } A \\ 0 & \text{otherwise} \end{cases}
& [\![i]\!]_\rho &= \rho(i) \quad (\text{a positional index as a value}) \\
[\![e_1\, e_2]\!]_\rho &= [\![e_1]\!]_\rho \otimes [\![e_2]\!]_\rho
& [\![\mathrm{sum}[J](e)]\!]_\rho &= \sum_{\sigma \in D_J} [\![e]\!]_{\rho[\sigma]} \\
[\![\mathrm{max}[J](e)]\!]_\rho &= \max_{\sigma \in D_J} [\![e]\!]_{\rho[\sigma]}
& [\![\mathrm{avg}[J](e)]\!]_\rho &= [\![\mathrm{sum}[J](e)]\!]_\rho \,/\, \lvert D_J \rvert \\
[\![\mathrm{maj}[J](e)]\!]_\rho &= \big[\, 2\,[\![\mathrm{sum}[J](e)]\!]_\rho > \lvert D_J \rvert \,\big]
& [\![f(e)]\!]_\rho &= \hat{f}_p\big([\![e]\!]_\rho\big)
\end{aligned}
```

```math
[\![\mathrm{softmax}[k](e, T)]\!]_\rho = \mathrm{RN}_p\!\left(\frac{e^{a(\rho(k))/T}}{\sum_{c \in D_k} e^{a(c)/T}}\right),
\qquad a(c) = [\![e]\!]_{\rho[k \mapsto c]}, \qquad T > 0
```

- `min` is defined like `max`. A projection over an empty domain is 0.
- `maj` takes a Boolean body, and a tie gives 0. The bracket $`[\,\cdot\,]`$ is 1 when its condition holds and 0 otherwise.
- `softmax[k](e)` has the temperature $`T = 1`$. [Section 2.5](#25-temperature) gives the temperature 0.
- `lnorm[k](e, ε)` is $`(a_k - \mu) \cdot \mathrm{RN}_p\big(1 / \sqrt{\sigma^2 + \varepsilon}\big)`$. The mean $`\mu`$ and the variance $`\sigma^2`$ are exact averages over $`D_k`$, and $`\varepsilon`$ is $`1/100000`$ by default. If $`\sigma^2 + \varepsilon \le 0`$, the value is $`\bot`$.
- $`\hat{f}_p`$ is $`f`$ for an exact function and $`\mathrm{RN}_p \circ f`$ for a correctly rounded function ([section 3](#3-correctly-rounded-functions)).

### 2.2 Statements

A statement $`s`$ has the form $`T[\lambda] \oplus{=}\, R`$.
Its contributions to the element $`\ell`$ of $`T`$ form a multiset:

```math
C_s(\ell) = \{\!\{\, [\![R]\!]_\sigma \;:\; \sigma \in D_L,\ [\![\lambda]\!]_\sigma = \ell \,\}\!\}
```

Here $`D_L`$ is the domain of the indices of the left side.
A defined tensor aggregates the contributions of all its statements $`S_T`$:

```math
\Theta'(T)(\ell) = \mathrm{agg}_{\oplus}\Big(\biguplus_{s \in S_T} C_s(\ell)\Big)
```

- The aggregates are sum, max and min. `avg` is the sum divided by the count, and `maj` is $`[\,2\,\mathrm{sum} > \mathrm{count}\,]`$. An empty multiset gives 0.
- A Boolean left side takes the step function $`H`$ of the aggregate.
- Facts and element lists contribute constants.

### 2.3 Programs

- The components of the dependency graph are evaluated in topological order.
- A component of class R0 is evaluated directly. A component of class R1 is evaluated by well-founded induction on its guard index.
- A component of class R2 takes the least fixpoint of its monotone operator ([Tar55](#ref-tar55)). The operator acts on the finite lattice of Boolean tensors over the active domain $`\mathcal{A}`$. Kleene iteration from the zero tensor reaches the fixpoint.
- Stratified negation gives the perfect model, stratum by stratum ([ABW88](#ref-abw88)).
- A virtual index has the denotation of an ordinary index, but only the slice at the largest index is observable.
- The output is the restriction of the tensors to the queries. A pattern query such as `R("Alice", x)?` gives a slice.

### 2.4 Scores

The score of a record $`r`$ is $`s(r) = [\![\mathit{score}]\!]_{x \mapsto r}`$.
A record with $`s(r) = \bot`$ is not eligible.
For a threshold $`\tau`$ and an optional list of ids, the eligible set is:

```math
E = \{\, r \;:\; s(r) \ne \bot,\ s(r) \ge \tau,\ \mathrm{id}(r) \in \mathit{ids} \,\}
```

The set is ordered by the score, from the highest, and then by the tie key $`\kappa`$, from the lowest.
This order instantiates the selection of `Similarity.v` with the scores in $`\mathbb{Q}`$ ([integration 2](integration.md#2-score-queries-tl-0)).

### 2.5 Temperature

For a temperature $`T > 0`$, the sigmoid $`\sigma_T(x) = 1/(1 + e^{-x/T})`$ is one correctly rounded function of the pair $`(x, T)`$.
The approved P6 study defines $`\sigma_0`$ as the step function.
The paper takes the limit, $`\lim_{T \to 0^{+}} \sigma_T = H_{1/2}`$, which is 1 above 0, one half at 0 and 0 below 0 ([Dom25](#ref-dom25), section 5). Open question K7 offers it.
For every $`T > 0`$, $`\sigma_T(s) \ge 1/2`$ holds exactly when $`s \ge 0`$, so a threshold of one half decides by the sign at every temperature.

At the temperature 0, `softmax[k]` takes its limit as $`T \to 0^{+}`$, which is exact.
Let $`M`$ be the positions of $`D_k`$ where $`a`$ is largest.
The value is $`1/\lvert M \rvert`$ at each position of $`M`$ and 0 elsewhere.
On the two logits $`x`$ and 0, this limit is $`H_{1/2}(x)`$, so `sig` and `softmax` agree at the temperature 0 under option (b) of K7.
A negative temperature fails with `TemperatureNegative` ([static semantics 2.3](static-semantics.md#23-calls)).

### 2.6 Built-in functions

The table gives the value and the domain of each built-in function.
Outside its domain, a function gives $`\bot`$, as division by zero does.
The column "Kind" says whether the function is exact or correctly rounded (CR, [section 3](#3-correctly-rounded-functions)).

| Function | Value | Domain | Kind |
|---|---|---|---|
| `step(x)` | $`H(x)`$: 1 if $`x > 0`$, and 0 otherwise | All of $`\mathbb{Q}`$ | Exact |
| `relu(x)` | $`\max(x, 0)`$ | All | Exact |
| `abs(x)` | $`\lvert x \rvert`$ | All | Exact |
| `sign(x)` | 1, 0 or −1, by the sign of $`x`$ | All | Exact |
| `min(x, y)`, `max(x, y)` | The smaller or the larger argument | All | Exact |
| `clamp(x, lo, hi)` | $`\min(\max(x, lo), hi)`$, as `numpy.clip` | All | Exact |
| `hardsig(x)` | $`\mathrm{clamp}(x/6 + 1/2, 0, 1)`$ | All | Exact |
| `exp(x)` | $`e^{x}`$ | All | CR |
| `log(x)` | The natural logarithm | $`x > 0`$ | CR |
| `sqrt(x)` | $`\sqrt{x}`$ | $`x \ge 0`$ | CR, by integer square roots |
| `rsqrt(x)` | $`1/\sqrt{x}`$ | $`x > 0`$ | CR, by integer square roots |
| `tanh(x)`, `sin(x)`, `cos(x)` | The hyperbolic tangent, the sine and the cosine, in radians | All | CR |
| `sig(x)`, `sig(x, T)` | $`\sigma_T(x)`$, with $`T = 1`$ by default ([section 2.5](#25-temperature)) | All | CR |
| `x / y` | $`x \cdot \mathrm{recip}(y)`$ | $`y \ne 0`$ | Exact |
| `x % y` | $`x - y \lfloor x / y \rfloor`$ | Integers $`x`$ and $`y`$, with $`y \ne 0`$ | Exact |
| `x ** y`, integer $`y`$ | $`x^{y}`$ | $`x \ne 0`$ or $`y \ge 0`$, and $`0^{0} = 1`$ | Exact |
| `x ** y`, other $`y`$ | $`x^{y}`$ | $`x > 0`$, or $`x = 0`$ and $`y > 0`$ | CR |

The exact functions are those of the approved P6 study, and `hardsig` keeps its slope of one sixth ([decision 52](../decision-register.md#decision-52)).
A negative base with an exponent that is not an integer gives $`\bot`$, as `numpy.power` gives NaN.

## 3. Correctly rounded functions

### 3.1 The functions

The correctly rounded (CR) functions are `exp`, `log`, `sqrt`, `rsqrt`, `tanh`, `sin`, `cos`, `sig(x, T)`, `softmax`, and `**` with an exponent that is not an integer.
Each returns $`\mathrm{RN}_p(f(x))`$, the rounding of the exact value to $`p`$ significant bits, ties to even ([decision 54](../decision-register.md#decision-54)).
The precision $`p`$ is between 8 and 256, and its default is 64.
A CR value is a dyadic number, so every computation after it stays exact.

### 3.2 Exact cases

Some inputs have an exact rational result, and the evaluator returns its rounding without a loop.

| Function | Exact cases |
|---|---|
| `exp` | $`e^{0} = 1`$ |
| `log` | $`\log 1 = 0`$ |
| `sin`, `tanh` | $`\sin 0 = \tanh 0 = 0`$ |
| `cos` | $`\cos 0 = 1`$ |
| `sig` | $`\sigma(0) = 1/2`$ |
| `softmax` | Equal arguments give $`1/n`$ for each of the $`n`$ elements |
| `**` | A rational power $`a^{m/n}`$ with an exact root, decided by integer roots |
| `sqrt`, `rsqrt` | Always exact by integer square roots, with no loop: the existing SQRT-ROUND (`rholang/src/rust/interpreter/tensor/num.rs:21-28`) |

### 3.3 The Ziv loop

For the other inputs, the evaluator runs a Ziv loop ([Ziv91](#ref-ziv91), [FHL07](#ref-fhl07)).
The loop starts at a working width a little above $`p`$.
It doubles the width until the rounding is decided or the width reaches its cap.

```text
CR(f, x, p, meter):                         ▷ the value RN_p(f(x)), defined by mathematics
  if EXACT-CASE(f, x): return RN_p(exact(f, x))
  w ← p + 32                                ▷ 32 guard bits
  loop
    reserve STEP-COST(f, x, w) on meter     ▷ decision 40: reserve before the work
    [lo, hi] ← ENCLOSE(f, x, w)             ▷ a rigorous dyadic interval: lo ≤ f(x) ≤ hi
    if RN_p(lo) = RN_p(hi): return RN_p(lo) ▷ RN_p is monotone, so this is RN_p(f(x))
    if w = 4p + 128: fail PrecisionExhausted
    w ← min(2w, 4p + 128)                   ▷ a fixed schedule, for example 96, 192, 384 for p = 64
```

Each step reserves its work on the meter before it encloses the value.
The schedule of widths depends only on $`p`$, so the static demand bound counts one step for each width ([static semantics 10](static-semantics.md#10-the-static-demand-bound)).

**Why the loop decides.** Take a rational input other than the exact cases.
The value of `exp`, `log`, `sin`, `cos`, `tanh` or `sig` at that input is transcendental, by the Lindemann–Weierstrass theorem ([Bak75](#ref-bak75)).
A rational power without an exact root is irrational.
An irrational value is never a rounding midpoint, which is a dyadic rational, so a narrow enough interval decides the rounding.

For `softmax`, a sum $`\sum_r m_r e^{r}`$ over distinct rationals with positive coefficients is transcendental unless every $`r`$ is 0.
The cap $`4p + 128`$ makes the loop end in every case, and an input that the cap cannot decide fails with `PrecisionExhausted`.

Figure TL9 shows the loop.

![Activity diagram of the Ziv loop. The loop first tests the exact cases and returns their rounding at once. Otherwise it sets the working width to p plus 32. In each round it reserves the cost of one step on the meter, which can exit with HostWorkRejected, then it encloses the value in a rigorous dyadic interval at the working width. If both ends of the interval round to the same p-bit value, that value is the correctly rounded result. Otherwise, if the width has reached the cap 4p plus 128, the loop exits with PrecisionExhausted, and if not, the width doubles, up to the cap, and the next round starts.](../diagrams/44-tl-ziv-loop.svg)

*Figure TL9. Source: [diagrams/44-tl-ziv-loop.puml](../diagrams/44-tl-ziv-loop.puml), rendered by [diagrams/render.sh](../diagrams/render.sh).*

## 4. Exact arithmetic

TL reuses the exact numbers of the P6 tensor module ([decision 54](../decision-register.md#decision-54)).

- **Ingress.** `number_of` (`rholang/src/rust/interpreter/tensor/num.rs:243-292`) and `vector_of` (`rholang/src/rust/interpreter/tensor/num.rs:325-360`) read the numbers of a `Par`. Integers, floats and Booleans become dyadic numbers, and `GBigRat` and `GFixedPoint` become reduced rationals. NaN and the infinities fail with `NonFiniteValue`, and the negative zero becomes 0.
- **Representation.** A value is a dyadic number or a reduced rational. A sum of dyadic products uses the long accumulator DYADIC-SUM (`rholang/src/rust/interpreter/tensor/exact.rs:44-52`), whose unit is the least exponent so far, so the sum never rounds.
- **Exact operations.** Division is exact, and so are `avg` and comparisons, which cross-multiply. `%` takes integer operands, and another operand gives $`\bot`$ ([section 2.6](#26-built-in-functions)).
- **Delivery.** A delivered score is $`\mathrm{RN}_{53}`$ of the exact score, by `round_to_f64` (`rholang/src/rust/interpreter/tensor/num.rs:465-482`).

## 5. The evaluation algorithm

### 5.1 The pipeline

The evaluator runs six phases before it computes anything.
DECODE reads the reflected term, RESOLVE binds the names, and INLINE expands the `let` definitions.
ELABORATE rewrites the sugar, SCOPE places the projections, and CANON computes the normal form ([GSLT 5](gslt.md#5-normalization)).
CHECK and PLAN then apply the static checks and compute the demand bound ([static semantics](static-semantics.md)).
EVALUATE runs last, and this section specifies it in literate form ([Knu84](#ref-knu84)): each chunk of pseudocode follows the prose that explains it.

### 5.2 Evaluating a program

The evaluator takes one strongly connected component at a time, in topological order.
So every tensor that a component reads is complete before the component starts.
Only the components that the queries need are evaluated ([section 6](#6-demand)).

```text
EVALUATE(NF, Θ_in, p, meter):
  for each component C of the query slice, in topological order:
    case class(C) = R0: for s in C: EVAL-STMT(s)
    case class(C) = R1: for g in D_g ascending: for s in C: EVAL-STMT(s restricted to the guard value g)
                        ▷ a virtual dimension keeps only the slices g and g + 1, with backing reserved once
    case class(C) = R2: SEMI-NAIVE(C)
  encode the outputs, and reserve each element before it is written
```

A component of class R1 visits its guard index in ascending order.
Each read in it is at a smaller guard value than the write, so the values that it reads are already final.

### 5.3 Evaluating a statement

A statement enumerates the valuations of its left indices in canonical order: positional values ascending, and atoms in the canonical order of `Par`.
For each valuation it evaluates the right side and folds the value into the accumulator of the left element.
The fold reserves its work before it runs.

```text
EVAL-STMT(T[λ] ⊕= R):
  for σ in D_L in canonical order:
    v ← EVAL(R, σ)
    reserve the fold, then fold (λ(σ), v) into accumulator(T, ⊕)
  finish the accumulators: avg = sum / count, maj, and H for a Boolean left side
```

### 5.4 Evaluating an expression

A product evaluates its factors in canonical order and stops at the first exact zero, because zero absorbs $`\bot`$.
A projection enumerates its bound indices with a generic join ([NPR18](#ref-npr18)).
The candidates of a variable are the intersection of the supports of the sparse factors that bind it.
A variable that no sparse factor binds takes its full domain.

```text
EVAL(e, ρ):
  case e = e1 e2 …:           evaluate the factors in canonical order; return 0 at the first exact 0
  case e = op[J](e'):         enumerate J by the generic join, in canonical order; aggregate EVAL(e', ρ[σ])
  case e = softmax[k](e', T): compute a(c) = EVAL(e', ρ[k ↦ c]) for every c in D_k, then CR softmax at T
                              ▷ at T = 0, the exact limit of section 2.5
  case e = lnorm[k](e', ε):   compute a(c) for every c in D_k, the exact mean and variance, then CR rsqrt
  case e = f(e'):             ⊥ outside the domain of f (section 2.6); otherwise apply f exactly,
                              or call CR(f, EVAL(e', ρ), p, meter)
```

### 5.5 Recursive rules

A component of class R2 runs the semi-naive iteration of Datalog ([BR86](#ref-br86)).
Each round evaluates the rules with at least one atom from the previous delta, so it never derives the same fact from old facts twice.
The iteration ends when a round derives no new tuple, so the number of rounds is at most the number of possible derived tuples.

```text
SEMI-NAIVE(C):
  R ← Δ ← the facts and the non-recursive rules of C
  while Δ ≠ ∅:
    N ← the rules of C evaluated with at least one atom from Δ, in canonical order
    Δ ← N − R
    R ← R ∪ Δ
```

Figures TL10 and TL11 show the order of the components and one semi-naive iteration.

![Activity diagram of the evaluation order of a TensorLogic program. The evaluator computes the query slice, the statements that the queries need, and its strongly connected components in topological order. For each component it branches on the class. A non-recursive component evaluates each statement once. An index-guarded component visits the guard values in ascending order and evaluates the statements at each value, and a virtual dimension keeps only two slices. A Boolean monotone component runs the semi-naive iteration, stratum by stratum. After the last component the evaluator encodes the query outputs and reserves each element before it writes it.](../diagrams/43-tl-evaluation-order.svg)

*Figure TL10. Source: [diagrams/43-tl-evaluation-order.puml](../diagrams/43-tl-evaluation-order.puml), rendered by [diagrams/render.sh](../diagrams/render.sh).*

![Activity diagram of the semi-naive iteration. The set R and the delta start with the facts and the non-recursive rules of the component. While the delta is not empty, the evaluator evaluates the rules with at least one atom of the delta, in canonical order, reserving the work of each join first. The new delta is the set of derived tuples that are not in R, and R grows by the delta. When the delta is empty, R is the least fixpoint, and the number of rounds is at most the number of possible derived tuples.](../diagrams/45-tl-semi-naive.svg)

*Figure TL11. Source: [diagrams/45-tl-semi-naive.puml](../diagrams/45-tl-semi-naive.puml), rendered by [diagrams/render.sh](../diagrams/render.sh).*

## 6. Demand

The evaluator computes only the static slice that the queries need.
This is the relevance of backward chaining in the paper ([Dom25](#ref-dom25), section 3.2), and the result equals the result of forward chaining.

## 7. Determinism, termination and resource bounds

**Determinism.** Native play and native replay compute the same results and reserve the same work, for five reasons.

1. The pipeline is a pure function of consensus inputs: the reflected term, the params, the records, the configured precision and the pinned Unicode data.
2. It uses no host floating point, no randomness, no clock, no cache and no iteration order of a hash map.
3. Every kernel is sequential ([I-9](../decision-register.md#i-9)).
4. Every reservation depends only on sizes and values.
5. A CR value is defined by mathematics, so a better algorithm can replace the Ziv loop with no fork.

**Termination.** Normalization is structural, and inlining is bounded.
Checking is finite, and the fixpoint of shapes is bounded by the hard dimensions.
Evaluation runs finite loops for class R0, $`\lvert D_g \rvert`$ steps for class R1, and at most one round for each possible tuple for class R2.
The Ziv loop has a capped width.

**Resource bound.** The reserved work is at most the static demand bound $`B`$.
A budget of at least $`B`$ never ends in `HostWorkRejected` ([verification plan 1](verification-plan.md#1-formal-models)).

## 8. Consensus state

| Consensus state | Not consensus state |
|---|---|
| TL terms, as reflected `Par` values, in deploys and params | The table of installed languages and its handles, which are local to one node process |
| Records and their embeddings | The static bound $`B`$ |
| The configured `precision` and the theory bytes of a space | The ChromaDB mirror |
| Content records under KIND `T` | |
| COMM events and replies, with their codes and fixed texts | |

TL outputs are not recorded processes: replay computes them again.

## References

<a id="ref-abw88"></a>[ABW88] K. R. Apt, H. A. Blair, and A. Walker, "Towards a theory of declarative knowledge," in *Foundations of Deductive Databases and Logic Programming*, Morgan Kaufmann, 1988, pp. 89–148. [doi:10.1016/B978-0-934613-40-8.50006-3](https://doi.org/10.1016/B978-0-934613-40-8.50006-3)

<a id="ref-bak75"></a>[Bak75] A. Baker, *Transcendental Number Theory*, Cambridge University Press, 1975. [doi:10.1017/CBO9780511565977](https://doi.org/10.1017/CBO9780511565977)

<a id="ref-br86"></a>[BR86] F. Bancilhon and R. Ramakrishnan, "An amateur's introduction to recursive query processing strategies," in *Proceedings of the 1986 ACM SIGMOD International Conference on Management of Data*, 1986, pp. 16–52. [doi:10.1145/16894.16859](https://doi.org/10.1145/16894.16859)

<a id="ref-dom25"></a>[Dom25] P. Domingos, "Tensor Logic: The Language of AI," arXiv:2510.12269v3, 2025. [doi:10.48550/arXiv.2510.12269](https://doi.org/10.48550/arXiv.2510.12269)

<a id="ref-fhl07"></a>[FHL07] L. Fousse, G. Hanrot, V. Lefèvre, P. Pélissier, and P. Zimmermann, "MPFR: A multiple-precision binary floating-point library with correct rounding," *ACM Transactions on Mathematical Software*, vol. 33, no. 2, article 13, 2007. [doi:10.1145/1236463.1236468](https://doi.org/10.1145/1236463.1236468)

<a id="ref-knu84"></a>[Knu84] D. E. Knuth, "Literate Programming," *The Computer Journal*, vol. 27, no. 2, pp. 97–111, 1984. [doi:10.1093/comjnl/27.2.97](https://doi.org/10.1093/comjnl/27.2.97)

<a id="ref-npr18"></a>[NPR18] H. Q. Ngo, E. Porat, C. Ré, and A. Rudra, "Worst-case optimal join algorithms," *Journal of the ACM*, vol. 65, no. 3, article 16, 2018. [doi:10.1145/3180143](https://doi.org/10.1145/3180143)

<a id="ref-tar55"></a>[Tar55] A. Tarski, "A lattice-theoretical fixpoint theorem and its applications," *Pacific Journal of Mathematics*, vol. 5, no. 2, pp. 285–309, 1955. [doi:10.2140/pjm.1955.5.285](https://doi.org/10.2140/pjm.1955.5.285)

<a id="ref-ziv91"></a>[Ziv91] A. Ziv, "Fast evaluation of elementary mathematical functions with correctly rounded last bit," *ACM Transactions on Mathematical Software*, vol. 17, no. 3, pp. 410–423, 1991. [doi:10.1145/114697.116813](https://doi.org/10.1145/114697.116813)
