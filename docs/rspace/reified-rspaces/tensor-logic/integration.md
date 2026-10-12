# TensorLogic integration

This document connects TensorLogic (TL) to the Reified RSpaces: to VectorDB spaces, to the capability `rho:lang:vector` and to the theory seam.
It also states how TL behaves under `where` guards, native play and native replay, merges, costs and errors.
The terms are defined in [concepts](concepts.md), and the VectorDB rules are in [specification 3.9](../specification.md#39-vectordb).

## Contents

1. [The fragments and their homes](#1-the-fragments-and-their-homes)
2. [Score queries (TL-0)](#2-score-queries-tl-0)
3. [Programs through rho:lang:vector (TL-1)](#3-programs-through-rholangvector-tl-1)
4. [Rules (TL-2)](#4-rules-tl-2)
5. [Embedding reasoning (TL-3)](#5-embedding-reasoning-tl-3)
6. [Admission theories](#6-admission-theories)
7. [Where guards](#7-where-guards)
8. [Play, replay and merges](#8-play-replay-and-merges)
9. [Costs](#9-costs)
10. [Errors](#10-errors)

## 1. The fragments and their homes

Each fragment of [decision 53](../decision-register.md#decision-53) runs in one place, with one FLT category.

| Fragment | Category | Where it runs | Section |
|---|---|---|---|
| TL-0, score queries | `tl:Score` | The VectorDB agent, inside a COMM | [2](#2-score-queries-tl-0) |
| TL-1, pure programs | `tl:Program` | The capability `rho:lang:vector`, method `eval` | [3](#3-programs-through-rholangvector-tl-1) |
| TL-2, Boolean rules | `tl:Program` | Rholang joins (TL-2a), or `eval` on a snapshot (TL-2b) | [4](#4-rules-tl-2) |
| TL-3, embedding reasoning | `tl:Score` and `tl:Program` | As TL-0 and TL-1 | [5](#5-embedding-reasoning-tl-3) |
| Admission | `tl:Check` | The theory seam of a space | [6](#6-admission-theories) |

All five paths use one TL engine: the decoder, the normalizer, the checker and the evaluator of [GSLT 5](gslt.md#5-normalization) and [dynamic semantics 5](dynamic-semantics.md#5-the-evaluation-algorithm).
The engine runs only in native execution, the one execution model of the campaign ([decision 69](../decision-register.md#decision-69)).

## 2. Score queries (TL-0)

### 2.1 The query vocabulary

A score query is a VectorDB receive whose query group holds the key `score` ([decision 65](../decision-register.md#decision-65)).
Three keys join the query vocabulary of [decision 56](../decision-register.md#decision-56).

| Key | Value | Rule |
|---|---|---|
| `score` | A `tl:Score` term, as a reflected value | It replaces `metric`. `metric` with `score` fails with `ConflictingParams`, and `radius` with `score` fails with `RadiusNeedsDistance`. |
| `tensors` | A map from names to tensors, in the value ABI of [syntax 3.3](syntax.md#33-how-data-enters) | It binds the named tensors of the score. A name must not shadow a reserved name (`ReservedName`). |
| `precision` | An integer from 8 to 256, 64 by default | The precision of the correctly rounded functions. It returns with TL, as [decision 66](../decision-register.md#decision-66) foresees. |

- `threshold` keeps its meaning, the least score of an eligible record. With `score`, its literal range relaxes from $`[0, 1]`$ to every rational, because a TL score has no fixed range (`models/src/rust/reified/validation.rs:172-226`).
- `k`, `results`, `ids` and `scores` do not change.
- A score that reads `q` needs a payload. Without one, the query fails with `QueryNeedsVector`. A score that does not read `q` needs no payload.

The reserved names of a score are `x` (the record) and `Emb[x, d]` (its embedding).
The others are `q[d]` (the payload of the query) and `D` (the dimensions of the space).
[Syntax 3.4](syntax.md#34-a-tl-score-in-a-vectordb-query) gives a complete query, which ranks the records by a bilinear form.

### 2.2 The query flow

The agent handles a score query in five steps.

1. `parse_query` (`rholang/src/rust/interpreter/spaces/disciplines/vectordb.rs:404-511`) reads the keys. Its list `QUERY_PARAMS` (lines 160 to 168 of that file) gains `score`, `tensors` and `precision`.
2. The engine decodes the term and normalizes it under the signature `Emb : [Sym, D]`, `q : [D]` and the shapes of `tensors`.
3. The checker requires a record-local score with no recursion ([static semantics 9](static-semantics.md#9-record-locality-of-scores)).
4. `eligible` (line 881 of `vectordb.rs`) evaluates the score of each record $`r`$. A record with $`s(r) = \bot`$ is not eligible. The other records with $`s(r) \ge \tau`$ form the eligible set $`E`$.
5. The agent orders $`E`$ by the score, from the highest, and then by the tie key $`\kappa`$ of [decision 41](../decision-register.md#decision-41). Then it selects as for a built-in metric.

The selection does not change.
Single mode, list mode, `ids` and `scores` work as in P6, and `scores` are delivered as $`\mathrm{RN}_{53}`$.
Parking, the cascade of list waiters and Rule E work as in P6 too.
A parked waiter keeps its query, with the reflected score term, in the policy slot of its continuation envelope ([decision 66](../decision-register.md#decision-66)).
So the size of the term counts toward the introduction bytes of the waiter.

Figure TL15 shows the query in native play and in native replay.

![Sequence diagram of a TL-0 score query. The deploy sends a receive with a query group that holds a tl:Score term to the VectorDB agent. The agent parses the query, and the TL engine decodes, normalizes and checks the term, reserving host work on the meter before each step. For each record of the collection, the engine evaluates the score with the exact core and the correctly rounded functions, and a record with an undefined score is skipped. The agent orders the eligible records by score and by the tie key, selects one record or a list, and commits the COMM to RSpace. In a parallel lane, native replay performs the same steps with the same reservations and reaches the same COMM, so the two meters agree.](../diagrams/49-tl-score-query.svg)

*Figure TL15. Source: [diagrams/49-tl-score-query.puml](../diagrams/49-tl-score-query.puml), rendered by [diagrams/render.sh](../diagrams/render.sh).*

### 2.3 Why the VectorDB proofs carry over

The proofs of the VectorDB discipline assume an eligibility function and an order that depend on each record alone.
These are the section variable `elig` and the section hypothesis `esort_insert`, which lines 228 and 232 of `VectorDbDiscipline.v` declare.
The record-locality lemma of [static semantics 9](static-semantics.md#9-record-locality-of-scores) gives both for a TL score.
So the theorem `vdb_no_pending_match` (`VectorDbDiscipline.v:662`) holds for score queries, and no parked waiter misses a record that it can take.

## 3. Programs through rho:lang:vector (TL-1)

A program calls `vec!?("eval", program, tensors, options)` and receives a reply first: `[true, outputs]` or `[false, code, detail]`.
The outputs map each query of the program to its value, in the value ABI.
[Syntax 4.16](syntax.md#416-a-tl-1-call) gives a complete call, and [GSLT 9](gslt.md#9-the-capability-rholangvector) gives every method.

The call is deterministic and is not recorded, as every operation of the capability.
Each phase of the engine reserves its work on the budget of the deploy before it runs (`rholang/src/rust/interpreter/reified/system_processes.rs:365-389`).

Figure TL16 shows an `eval` call.

![Sequence diagram of an eval call of rho:lang:vector. The deploy sends the method name, a tl:Program term, the map of tensors and the options to the capability on channel E0 23. The capability unpacks the arguments, and the TL engine decodes the term, checks its fingerprint, normalizes and checks it, and evaluates the queries, reserving host work on the meter before each phase. A failure at any phase gives the reply false with a code and a detail. On success the capability encodes each query value in the value ABI and sends the reply true with the map of outputs to the return channel.](../diagrams/50-tl-vector-eval.svg)

*Figure TL16. Source: [diagrams/50-tl-vector-eval.puml](../diagrams/50-tl-vector-eval.puml), rendered by [diagrams/render.sh](../diagrams/render.sh).*

## 4. Rules (TL-2)

A Boolean rule can run in two ways.

**TL-2a: rules as Rholang joins.** The rule `Aunt(x, z) <- Sister(x, y), Parent(y, z)` is the peek join `for (@[x, y] <<- @"Sister" & @[y2, z] <<- @"Parent" where y == y2) { @"Aunt"!([x, z]) }` in one space.
Rholang patterns are linear, so the shared variable `y` needs the guard. Each firing derives one fact.

The inner collection of the space chooses the semiring.
A set keeps each fact once, which is the step function of a rule, and a bag counts the derivations.
The current join search fills the positions in order and checks the guard once (rule JN-8 of [specification 3.8](../specification.md#38-joins)).
So it can park while a witness exists.
Under the selection rule of [decision 37](../decision-register.md#decision-37) at P12, the join fires whenever a witness exists.

**TL-2b: rules set at a time.** A program reads a snapshot of a relation and evaluates all its rules in one `eval` call.
The snapshot is one value: a relation in a cell, read with a peek, or a list read of a VectorDB collection of facts.
The engine evaluates the rules by semi-naive iteration ([dynamic semantics 5](dynamic-semantics.md#5-the-evaluation-algorithm)) and replies with the derived facts.
[Syntax 4.17](syntax.md#417-rules-set-at-a-time-tl-2b) gives a complete example.

A join bind in list mode still fails with `ListModeInJoin` ([decision 66](../decision-register.md#decision-66)), so a list read of facts cannot sit in a join. Open question K9 keeps this rule.

Figure TL17 shows TL-2b.

![Sequence diagram of rules set at a time. The deploy peeks the relation from a cell, or reads a list of facts from a VectorDB collection, and so holds one snapshot of the relation. It calls eval of rho:lang:vector with a tl:Program of rules and the snapshot as a tensor. The TL engine checks that the rules are safe and stratified, then runs semi-naive iteration: each round joins the new facts of the last round with the rules, until no new fact appears. The capability replies with the derived facts of the queries, and the deploy can send them to a space.](../diagrams/51-tl-set-at-a-time.svg)

*Figure TL17. Source: [diagrams/51-tl-set-at-a-time.puml](../diagrams/51-tl-set-at-a-time.puml), rendered by [diagrams/render.sh](../diagrams/render.sh).*

## 5. Embedding reasoning (TL-3)

Embedding reasoning uses the sigmoid with a temperature, $`\sigma_T`$ ([dynamic semantics 2.5](dynamic-semantics.md#25-temperature)).
A score query reasons over the record embeddings, with embedded relations passed in `tensors`, as the last row of [syntax 4.13](syntax.md#413-vectordb-scores-tl-0) shows.
A program reasons over a whole knowledge base through `eval`, as [syntax 4.12](syntax.md#412-embedding-reasoning-paper-section-5) shows.
At every temperature above 0, the threshold one half decides by the sign of the argument.
So a threshold query reads the same facts as a deduction.

## 6. Admission theories

A space can admit values with a TL check term: `free TensorCheck(tl, tl:Check{…}, p)` ([GSLT 10](gslt.md#10-admission-theories)).
The theory works through the three points of the theory seam ([theory seam](../theory-seam.md)).

1. **At creation.** The factory resolves the identity of the theory and stores it as compact bytes in the config of the space ([decision 74](../decision-register.md#decision-74)). The canonical normal form of the check term goes to a content record under the KIND `T` (open question K8).
2. **At a send.** Each value of the send is checked before any staging. `Refuted` and `Undetermined` abort the deploy, so `Undetermined` fails closed.
3. **At a match.** Each stored datum is checked again in the COMM. A verdict other than `Proven` makes the datum ineligible, as a guard rejection does.

Every check reserves its work before it reads the value, as the theory seam requires ([theory seam 9](../theory-seam.md#9-charges)).
A check is a pure function of the identity, the value and the budget.
So play, replay and a later attach give the same verdicts in the same order.

Figure TL18 shows the three points.

![Sequence diagram of TL admission. At creation, the deploy calls a factory with free TensorCheck and a tl:Check term, the factory asks the theory seam to resolve the identity, the TL engine normalizes the check term and computes the hash of its normal form, and the factory stores the compact identity bytes in the config of the space and the normal form under KIND T. At a send, the space agent reads the identity from the config, the TL engine evaluates the check on each value after reserving its work, and a refuted or undetermined verdict aborts the deploy before staging. At a match, the agent checks each stored datum again, and a verdict other than proven makes the datum ineligible.](../diagrams/52-tl-admission.svg)

*Figure TL18. Source: [diagrams/52-tl-admission.puml](../diagrams/52-tl-admission.puml), rendered by [diagrams/render.sh](../diagrams/render.sh).*

## 7. Where guards

A TL score filters the eligible set $`E`$ itself, so it always takes part in the selection.
Until P12, a `where` guard follows the current rule of [decision 37](../decision-register.md#decision-37): the guard is checked once, on the first spatial match.

At P12, the space adopts the rule of F1r3lang.
The candidates are taken in the order of score and tie key.
The first candidate that matches, passes the guard and passes the theory check fires.

A `where` guard cannot evaluate TL, because guards are predicates of F1r3lang.
A guard can test a TL term only by a structural FLT pattern.

## 8. Play, replay and merges

- **Play and replay.** TL evaluation is a pure computation. Native replay evaluates again, with the same reservations as native play, so the meters agree.
- **Records.** The COMMs of a score query are the COMMs of a P6 query. TL adds no entry to `non_deterministic_ops`.
- **Merges.** A score query reads the collection and its query, as a P6 query does. So merges behave as in P6, and the soft bound of `capacity` applies ([decision 71](../decision-register.md#decision-71)).

## 9. Costs

The engine reserves every step of its work before the step, as [decision 40](../decision-register.md#decision-40) requires.
The demand bound $`B`$ is advisory: the method `demand` reports it, and a budget of at least $`B`$ never ends in `HostWorkRejected`.

At the host-work limits of protocol 6, native play of 8 records and 8 list waiters already uses 0.54 of the limit of `VerificationBytes`.
Native replay costs much more ([specification 3.9](../specification.md#39-vectordb), rule VD-14).
[Decision 79](../decision-register.md#decision-79) keeps these limits while the replay costs are repaired.
Until then, a TL score fits only a small collection.

## 10. Errors

- A static error of a TL term in a query aborts the deploy before any staging, as a P6 query error does. The error is a typed `SpaceError`.
- A record whose score is $`\bot`$ is not eligible, and the query goes on.
- `PrecisionExhausted` while the query scores a record aborts the deploy, as approved. Open question K2 recommends that such a record becomes ineligible instead.
- A failure in `eval` gives the reply `[false, code, detail]`, with the codes of [static semantics 12](static-semantics.md#12-diagnostics) and `UnknownMethod` and `BadArgument`.
