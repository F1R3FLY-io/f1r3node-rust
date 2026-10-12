# TensorLogic concepts

This document defines the terms of TensorLogic (TL) and explains the idea of the language.
The other TL documents use these terms without a new definition.
The general terms of the campaign are in the [glossary](../glossary.md), for example [space](../glossary.md#space), [datum](../glossary.md#datum), [record envelope](../glossary.md#record-envelope) and [host work](../glossary.md#host-work).

## Contents

1. [The idea: one construct for logic and for tensors](#1-the-idea-one-construct-for-logic-and-for-tensors)
2. [Tensors, indices and shapes](#2-tensors-indices-and-shapes)
3. [Joins and projections](#3-joins-and-projections)
4. [Equations, accumulation and queries](#4-equations-accumulation-and-queries)
5. [Rules, facts and Boolean tensors](#5-rules-facts-and-boolean-tensors)
6. [Normalization, virtual indices and embeddings](#6-normalization-virtual-indices-and-embeddings)
7. [Exact numbers and correct rounding](#7-exact-numbers-and-correct-rounding)
8. [Languages, terms and reflection](#8-languages-terms-and-reflection)
9. [Fragments, costs and identities](#9-fragments-costs-and-identities)
10. [Symbols](#10-symbols)

The [references](#references) follow the last section.

## 1. The idea: one construct for logic and for tensors

Domingos observes that a logic rule and a tensor contraction are the same operation ([Dom25](#ref-dom25), section 2).
A relation is a tensor of zeros and ones.
A rule joins relations and then projects out the variables that its head does not keep.
A numeric tensor equation does the same with numbers: it multiplies tensors on their shared indices and sums over the other indices.
So TL has one construct, the *tensor equation*, and a program is a set of equations.

Example: the rule "x is a grandparent of z when x is a parent of y and y is a parent of z" is the Boolean equation below.
The product joins the two relations on the shared index $`y`$.
The step function $`H`$ then turns the count of the paths into a truth value.

```math
\mathrm{Grandparent}_{xz} = H\Big(\sum_{y} \mathrm{Parent}_{xy}\,\mathrm{Parent}_{yz}\Big)
```

The same shape of equation computes a layer of a neural network, with a numeric weight tensor in place of a relation.
This uniformity is the reason TL fits a VectorDB space: one language scores vectors, applies rules and combines both.

## 2. Tensors, indices and shapes

**Tensor.** A tensor $`T`$ of rank $`r`$ is a finite map from index tuples to values:

```math
T : D_{1} \times \dots \times D_{r} \to \mathbb{Q}_{\bot}
```

Here $`D_k`$ is the domain of the $`k`$-th index, and $`\mathbb{Q}_{\bot}`$ is the set of rational numbers with one extra value $`\bot`$ (undefined).
An element that a program does not define is 0, as in the paper ([Dom25](#ref-dom25), page 6).

**Index.** An index is a named variable such as `i`, `j` or `x`.
It ranges over a finite domain of one of two kinds.

| Domain kind | Values | Example |
|---|---|---|
| Positional | The integers $`\{0, \ldots, n-1\}`$ | The 384 positions of an embedding |
| Symbolic | The *active domain* $`\mathcal{A}`$: the finite set of atoms in the inputs and in the program | The names of people in a family relation |

**Atom.** An atom is a ground Rholang value that names a symbolic index value, for example the string `"Alice"`.
Atoms are ordered by the canonical order of `Par`, so every enumeration over a symbolic domain has one order.

**Shape.** The shape of a tensor lists its dimensions.
A dimension is `Pos(n)` for a positional index of size $`n`$, or `Sym` for a symbolic index.
A *hard* dimension comes from input data or a declaration, and a *soft* dimension is an inferred bounding box.

**Size variable.** A declaration such as `X : [N, D]` names the sizes of the dimensions of `X`.
Each name in it, here `N` and `D`, is a *size pattern*, which binds a *size variable*.
A size variable is a constant equal to the size of its dimension, so `sqrt(D)` is a number.
The reserved name `D` of a score is a size variable too ([syntax 3.3](syntax.md#33-how-data-enters)).

**Kind.** Every tensor has the kind `Num` (rational values) or `Bool` (values 0 and 1).
`Bool` is a subkind of `Num`, so a Boolean tensor can enter arithmetic.

## 3. Joins and projections

**Join.** The join of two tensors multiplies them on their shared indices and keeps every index ([Dom25](#ref-dom25), page 5).
For tensors $`U_{\alpha\beta}`$ and $`V_{\beta\gamma}`$, where $`\alpha`$, $`\beta`$ and $`\gamma`$ are lists of indices:

```math
(U \bowtie V)_{\alpha\beta\gamma} = U_{\alpha\beta}\, V_{\beta\gamma}
```

TL writes a join by *juxtaposition* (two terms side by side, `U[a, b] V[b, c]`) or with `*`.

**Projection.** A projection keeps some indices and aggregates over the others.
For the aggregation operator $`\oplus`$:

```math
\pi^{\oplus}_{\alpha}(T)(a) = \bigoplus_{b} T(a, b)
```

The operator $`\oplus`$ is one of sum, max, min, avg (average) and maj (majority).
An equation projects its right side onto the indices of its left side, so the indices that the left side does not name are aggregated.
The *scope rule* decides where inside the right side that aggregation happens ([static semantics 3](static-semantics.md#3-the-scope-rule)).

Figure TL2 shows a join and a projection on small tensors.

![Diagram of one join and one projection. The tensor U with indices a and b and the tensor V with indices b and c enter a join, which multiplies them where the shared index b agrees and gives a tensor with indices a, b and c. A projection onto a and c then sums over b. The result is the matrix product of U and V. The same two steps with the step function applied after the sum compute the Boolean composition of two relations.](../diagrams/36-tl-join-projection.svg)

*Figure TL2. Source: [diagrams/36-tl-join-projection.puml](../diagrams/36-tl-join-projection.puml), rendered by [diagrams/render.sh](../diagrams/render.sh).*

## 4. Equations, accumulation and queries

**Equation.** An equation has the form `LHS ⊕= RHS`.
The left side names a tensor and its indices, and the right side is an expression.
The token `=` means `+=`: the sum projection.
The tokens `max=`, `min=`, `avg=` and `maj=` select the other aggregation operators.

**Accumulation.** Equations with the same left-side tensor accumulate with their operator ([Dom25](#ref-dom25), page 6).
So `Y[i] = A[i] | Y[i] = B[i]` defines $`Y_i = A_i + B_i`$.

**Program.** A program is a set of statements joined by `|`, the parallel composition of Rholang.
`E1 | E2` means that both statements hold, and the order of the statements does not matter.

**Query.** A query `T?` marks a tensor as an output, and `R("Alice", x)?` asks for a slice of a relation ([Dom25](#ref-dom25), page 6).
Evaluation computes only what the queries need.

## 5. Rules, facts and Boolean tensors

**Boolean tensor.** A Boolean tensor, also called a relation, holds the values 0 and 1.
TL reads an access with parentheses, `R(x, y)`, as Boolean: "parentheses mean Boolean" ([Dom25](#ref-dom25), page 6).

**Step.** The step function $`H`$ maps a positive number to 1 and every other number to 0:

```math
H(x) = \begin{cases} 1 & x > 0 \\ 0 & \text{otherwise} \end{cases}
```

**Fact.** A fact is a ground Boolean statement, for example `Parent("Alice", "Bob")`.
It contributes the value 1 at its position.

**Rule.** A rule `H(x̄) <- L1, …, Ln` is the Boolean equation `H(x̄) = L1 … Ln` ([Dom25](#ref-dom25), pages 2 to 5).
The body literals join, the projection sums over the variables that the head does not keep, and $`H`$ turns the sum into a truth value.
A set of recursive rules has a least fixpoint, which is the meaning of Datalog ([CGT89](#ref-cgt89)).

## 6. Normalization, virtual indices and embeddings

**Normalized index.** The mark `.` after a left-side index, as in `Comp[p, p'.]`, normalizes along that index ([Dom25](#ref-dom25), page 10).
`softmax` and `lnorm` (layer normalization) are the two normalizations.

**Virtual index.** The mark `*` before an index, as in `X[*t]`, keeps no memory along that index ([Dom25](#ref-dom25), page 7).
Each new slice replaces the previous one, as in the state of a recurrent network.

**Embedding.** An embedding is the numeric vector of a record of a VectorDB space.
In a TL score, the reserved name `Emb[x, d]` is the embedding of the record `x`.

**Temperature.** The temperature $`T \ge 0`$ controls how hard a sigmoid decides ([Dom25](#ref-dom25), section 5):

```math
\sigma_T(x) = \frac{1}{1 + e^{-x/T}}
```

A low temperature makes $`\sigma_T`$ close to a step, so reasoning in embedding space becomes deductive.
A higher temperature lets similar objects share inferences.

## 7. Exact numbers and correct rounding

**Exact domain.** TL computes in the rationals $`\mathbb{Q}`$ plus $`\bot`$, as [decision 54](../decision-register.md#decision-54) requires.
A *dyadic* number has the form $`m \cdot 2^{e}`$ with integers $`m`$ and $`e`$, and every finite IEEE 754 value is dyadic ([IEEE19](#ref-ieee19)).

**Rounding.** $`\mathrm{RN}_p(x)`$ rounds $`x`$ to the nearest number with a $`p`$-bit significand, with ties to the even neighbor and no exponent limit.
A function value is *correctly rounded* (CR) when it equals $`\mathrm{RN}_p(f(x))`$, the rounding of the exact mathematical value.

**Ziv loop.** A Ziv loop computes a correctly rounded value ([Ziv91](#ref-ziv91)).
It encloses $`f(x)`$ in a rigorous interval at a working width.
A larger width narrows the interval, and the loop raises the width until both ends round to the same value.
TL caps the width, so the loop always ends ([dynamic semantics 3](dynamic-semantics.md#3-correctly-rounded-functions)).

## 8. Languages, terms and reflection

**GSLT.** A graph-structured lambda theory is a finite presentation of a language: its grammar, its equations and its rewrites ([SM17](#ref-sm17)).
TL is a GSLT that the node installs in F1r3lang.

**OSLF.** Operational semantics in logical form is the logic or type system that the rewrites of a GSLT generate ([SM16](#ref-sm16), [WS22](#ref-ws22)).

**FLT.** A foreign-language term is a scoped term of an installed guest language inside Rholang 1.4.
Its form is `handle:Category{…}`, where the handle names the installed language and the category names a sort of its grammar.

**Handle.** The handle is the opaque capability of an installed language.
A program obtains it from the language installer of F1r3lang.

**Reflection.** F1r3lang turns a guest term into a Rholang `Par`.
Each constructor becomes an `EList` whose head is the unforgeable name `GPrivate("mettail.term.{F}.{Label}")`, so a term can travel as a value.
Quoting terms into names follows the reflective higher-order calculus ([MR05](#ref-mr05)).

**Fingerprint.** The fingerprint $`\mathcal{F}`$ is the hash of the canonical definition of a language.
It is part of every reflected tag, so a node can check which language a term belongs to.

**Normal form.** The normal form (NF) of a TL term is the canonical core term that the normalizer computes ([GSLT 5](gslt.md#5-normalization)).
Two terms that mean the same computation have one NF.

## 9. Fragments, costs and identities

**Fragments.** [Decision 53](../decision-register.md#decision-53) names four fragments of TL for VectorDB spaces.

| Fragment | Content | Where it runs |
|---|---|---|
| TL-0 | Score queries: a TL expression gives each record a score, and the query selects by the score | Inside the VectorDB agent, in a COMM |
| TL-1 | Pure programs | The capability `rho:lang:vector` |
| TL-2 | Boolean rules, as Rholang joins (TL-2a) or set at a time through the capability (TL-2b) | Joins, or the capability |
| TL-3 | Embedding reasoning with a temperature | Scores and programs |

**Host work.** Host work is the deploy budget of the node over verification operations, verification bytes and search-state bytes ([decision 40](../decision-register.md#decision-40)).
TL reserves host work before each step of work.

**Demand bound.** The demand bound $`B`$ is a static upper bound on the host work that a term reserves.
The checker computes it from the NF and the input shapes ([static semantics 10](static-semantics.md#10-the-static-demand-bound)).

**Content identity.** The content identity of a theory is its eight-field identity, stored as compact bytes in the config of a space ([decision 74](../decision-register.md#decision-74)).

**Tie key.** The tie key $`\kappa`$ breaks ties between records with equal scores: first the hash of the source produce, then the full digest ([decision 41](../decision-register.md#decision-41)).

## 10. Symbols

| Symbol | Meaning |
|---|---|
| $`\bowtie`$ | Join |
| $`\pi^{\oplus}_{\alpha}`$ | Projection onto the indices $`\alpha`$ with the operator $`\oplus`$ |
| $`\oplus`$ | An aggregation operator: sum, max, min, avg or maj |
| $`H`$ | The step function |
| $`\sigma_T`$ | The sigmoid at temperature $`T`$ |
| $`\mathbb{Q}_{\bot}`$ | The rationals with the undefined value $`\bot`$ |
| $`\mathcal{A}`$ | The active domain of atoms |
| $`D_J`$ | The domain of the list of indices $`J`$: the product of their domains |
| $`\mathrm{RN}_p`$ | Rounding to $`p`$ significant bits, ties to even |
| $`\mathcal{F}`$ | The fingerprint of a language |
| $`B`$ | The static demand bound |
| $`\kappa`$ | The tie key of decision 41 |

## References

<a id="ref-cgt89"></a>[CGT89] S. Ceri, G. Gottlob, and L. Tanca, "What you always wanted to know about Datalog (and never dared to ask)," *IEEE Transactions on Knowledge and Data Engineering*, vol. 1, no. 1, pp. 146–166, 1989. [doi:10.1109/69.43410](https://doi.org/10.1109/69.43410)

<a id="ref-dom25"></a>[Dom25] P. Domingos, "Tensor Logic: The Language of AI," arXiv:2510.12269v3, 2025. [doi:10.48550/arXiv.2510.12269](https://doi.org/10.48550/arXiv.2510.12269)

<a id="ref-ieee19"></a>[IEEE19] "IEEE Standard for Floating-Point Arithmetic," IEEE Std 754-2019, 2019. [doi:10.1109/IEEESTD.2019.8766229](https://doi.org/10.1109/IEEESTD.2019.8766229)

<a id="ref-mr05"></a>[MR05] L. G. Meredith and M. Radestock, "A Reflective Higher-order Calculus," *Electronic Notes in Theoretical Computer Science*, vol. 141, no. 5, pp. 49–67, 2005. [doi:10.1016/j.entcs.2005.05.016](https://doi.org/10.1016/j.entcs.2005.05.016)

<a id="ref-sm16"></a>[SM16] M. Stay and L. G. Meredith, "Logic as a distributive law," arXiv:1610.02247, 2016. [doi:10.48550/arXiv.1610.02247](https://doi.org/10.48550/arXiv.1610.02247)

<a id="ref-sm17"></a>[SM17] M. Stay and L. G. Meredith, "Representing operational semantics with enriched Lawvere theories," arXiv:1704.03080, 2017. [doi:10.48550/arXiv.1704.03080](https://doi.org/10.48550/arXiv.1704.03080)

<a id="ref-ws22"></a>[WS22] C. Williams and M. Stay, "Native Type Theory," *Electronic Proceedings in Theoretical Computer Science*, vol. 372, pp. 116–132, 2022. [doi:10.4204/EPTCS.372.9](https://doi.org/10.4204/EPTCS.372.9)

<a id="ref-ziv91"></a>[Ziv91] A. Ziv, "Fast evaluation of elementary mathematical functions with correctly rounded last bit," *ACM Transactions on Mathematical Software*, vol. 17, no. 3, pp. 410–423, 1991. [doi:10.1145/114697.116813](https://doi.org/10.1145/114697.116813)
