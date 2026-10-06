# Reified RSpaces specification

This specification states the required behavior of the native spaces of Reified RSpaces.
A [native space](glossary.md#native-space) is a [space](glossary.md#space) that the node implements, and a Rholang program creates it with a [factory](glossary.md#factory).
Each rule below states what a correct implementation must do, and not how the code does it.
The [design](design.md#native-spaces) explains how the current code meets each rule.
The [agent API](agent-api.md) states the calls, the replies, and the errors that a program sees.

The [decision register](decision-register.md) is normative, and this specification follows it.
Where this specification and the register differ, the register applies.

## Contents

- [Scope of this version](#scope-of-this-version)
- [Terms and notation](#terms-and-notation)
- [1. Space model](#1-space-model)
- [2. Outer stores](#2-outer-stores)
- [3. Inner collections](#3-inner-collections)
- [4. Qualifiers](#4-qualifiers)
- [5. Theories](#5-theories)
- [6. Constructs that fail closed](#6-constructs-that-fail-closed)
- [7. Conformance](#7-conformance)
- [References](#references)

## Scope of this version

Phase P4 writes the rules of the first [space family](glossary.md#space-family), `hashmap:queue`:

- the [space model](#1-space-model), which every family shares
- the [HashMap outer store](#21-hashmap)
- the [rules of ordered collections](#30-ordered-collections) and the [queue discipline](#31-queue)
- the [default qualifier](#41-default)

Later phases add their rules as new sections at the places that Table 1 marks.
Each new section must state the items that its extension point lists, so that a reader can compare the families.

*Table 1. The sections that later phases add.*

| Phase | Section | Content |
|---|---|---|
| P5 | [2.2](#22-outer-stores-of-later-phases) | The PathMap, Array, Vector, and HashSet outer stores |
| P5 | [3.2](#32-disciplines-of-later-phases) | The bag, set, stack, cell, and priority queue disciplines, the query of a priority queue, and joins on ordered collections ([decision 29](decision-register.md#decision-29)) |
| P6 | [3.2](#32-disciplines-of-later-phases) | The VectorDB discipline, its query, and its bounds |
| P7 | [4.2](#42-qualifiers-of-later-phases) and [5](#5-theories) | The `temp` and `seq` qualifiers, the sweep of their state, and the theories |
| P8 | [6](#6-constructs-that-fail-closed) | Spaces that user agents implement |

## Terms and notation

The [glossary](glossary.md) defines each linked term.
Table 2 defines the symbols of the rules.
A sentence with the word "must" states a requirement.

*Table 2. The symbols of this specification.*

| Symbol | Meaning |
|---|---|
| $`s`$ | A native space. |
| $`\mathit{sid}`$ | The [space id](glossary.md#space-id) of $`s`$: 32 bytes. |
| $`c`$ | A channel: a `Par` value, after the reducer removes its bundle. |
| $`k = \mathrm{key}(c)`$ | The [collection key](glossary.md#collection-key) of the channel $`c`$ in the outer store of $`s`$. |
| $`R`$ | The records of one [collection](glossary.md#collection): a finite set of pairs $`(o, d)`$ of an order $`o`$ and a datum $`d`$. |
| $`W`$ | The waiters of one collection: a finite set of pairs $`(o, w)`$ of an order $`o`$ and a waiting continuation $`w`$. |
| $`v`$ | The value of the [collection counter](glossary.md#collection-counter) of a collection. |
| $`\mathrm{match}(w, d)`$ | True when the pattern of $`w`$ matches the payload of $`d`$ and the guard of $`w`$ accepts the bindings. |
| $`\mathrm{head}(X)`$ | The entry of the side $`X`$ that interacts next. Rule QD-1 defines it for a queue. |

## 1. Space model

### 1.1 Spaces

**Rule SM-1: identity.** A native space has a space id of 32 bytes.
The factory derives the id from the random state $`r`$ of the call ([decision I-19](decision-register.md#i-19)):

```math
\mathit{sid} = \mathrm{next}\bigl(\mathrm{split}_{\mathtt{0x01}}(\mathrm{split}_{\mathtt{0xE0}}(r))\bigr)
```

A second creation with one random state gives the same id, so it must abort the deploy with `SpaceAlreadyExists`.
The abort is the same in play and in replay.

**Rule SM-2: configuration.** A native space has exactly one [config record](glossary.md#config-record).
The record holds the map of Table 3, and nothing changes the map after the creation ([decision I-17](decision-register.md#i-17)).
A collection operation, a handle method, and `getSpaceAgent` read the map, and none of them writes it.

*Table 3. The entries of the config map.*

| Key | Value | Value in P4 |
|---|---|---|
| `"spaceId"` | A byte array of 32 bytes | The space id |
| `"outer"` | A string that names the outer store | `"hashmap"` |
| `"inner"` | A string that names the inner collection | `"queue"` |
| `"qualifier"` | A string that names the qualifier | `"default"` |
| `"params"` | A map from the named factory params to their values | `{}`, because the family takes no param |
| `"theory"` | The theory of the space | `Nil` until P7 |

**Rule SM-3: existence.** An operation on a space without a config record must abort the deploy.
The code is `DanglingName` when the program reaches the space through a space name, and `DanglingHandle` in every other case.
So every operation has a defined result, also on a reference whose space does not exist.

### 1.2 Collections

**Rule SM-4: collections.** A native space holds one collection for each collection key.
The outer store of the family maps each channel to its key (section [2](#2-outer-stores)).
A collection exists from the first operation on it, and it never needs a separate creation.

**Rule SM-5: isolation.** An operation on a collection reads the config record of its space.
It changes only its collection, the counter of that collection, and, on the first operation, the [space counter](glossary.md#space-counter).
Two collections share no record and no waiter.

### 1.3 Entries and operations

**Rule SM-6: entries.** A collection holds records and waiters.
A record is a datum with its order, and a waiter is a waiting continuation with its order.
A record keeps the payload, the persistence flag, and the random state that the operation gave it.
A waiter has exactly one bind, because no join reads a collection in P4.

**Rule SM-7: operations.** A program reaches a collection with two operations, which Table 4 lists.
Each operation has the persistence flag of its form, and a consume also has a peek flag.

*Table 4. The operations on a collection.*

| Operation | Program forms | Flags |
|---|---|---|
| Produce | `c!(d…)`, `c!!(d…)`, and the handle method `produce`, also with `persistent=true` | persistent |
| Consume | `for (p <- c)`, `for (p <= c)`, and `for (p <<- c)` | persistent (`<=`), peek (`<<-`) |

**Rule SM-8: atomicity.** Each operation runs as one [space transaction](glossary.md#space-transaction), which commits all its effects or none ([decision 19](decision-register.md#decision-19)).
The events, the charges, the replay, and the merge of the operation are those of the equivalent host operations.
An abort of the deploy reverts every operation of the deploy, as for host operations.

**Rule SM-8a: legacy charges.** In a legacy deploy, each staged operation of a space transaction pays the [legacy charges](glossary.md#legacy-charge) of its equivalent host operation ([decision 45](decision-register.md#decision-45), Option P).
The pre-charge of an operation runs before the observer calls of the operation.
Its post-charges run before the next operation, and the post-charges of the last operation run after the commit.
So the legacy log of a space operation is the log of the equivalent host program.
A charge that runs out of phlo stops the operation where the host program stops, in play and in replay.

**Rule SM-9: determinism.** The result of an operation depends only on the operation, its random state, and the committed state of its collection.
It must not depend on the list order of the store, on a restart, or on the timing of threads.

### 1.4 Routing

**Rule SM-10: routing.** The reducer gives each channel exactly one [routing class](glossary.md#routing-class), and the class decides the space of the channel.
Table 5 states the rules, and section [N5 of the design](design.md#n5-routing-and-scope) shows their algorithm.
A program that uses no Reified syntax and holds no space name must route every channel to the default space.

*Table 5. The routing rules.*

| Rule | Channel | Space |
|---|---|---|
| R1 | A byte name, an op channel, a config, handle, matcher, or body id, or an ordinary unforgeable name | The default space |
| R2 | A storage channel: a data, path-index, allocator, or counter id | The default space |
| R3 | A [space name](glossary.md#space-name) of the space $`s`$ | The collection $`\mathrm{key}(c)`$ of $`s`$, in every [use context](glossary.md#use-context) |
| R4 | A name of a user agent | The user agent, which fails closed until P8 |
| R5 | Any other channel, such as `@"jobs"` | The collection $`\mathrm{key}(c)`$ of the space of the use context, or the default space without a use context |
| R6 | A channel that routes to a collection of a native space | The operation uses the data channel $`D(\mathit{sid}, k)`$ of the collection |
| R7 | The reply channel of a system process | The rules R1 to R5 without a use context |

## 2. Outer stores

An [outer store](glossary.md#outer-store) maps the channels of a space to collection keys.

### 2.1 HashMap

**Rule HM-1: collection key.** The key of a channel is the bincode encoding of its sorted form ([decision 32](decision-register.md#decision-32), item A5):

```math
\mathrm{key}(c) = \mathrm{bincode}\bigl(\mathrm{sort}(c)\bigr)
```

Here $`\mathrm{sort}`$ is the normal form of `ParSortMatcher`, which is also the preimage of the store identity of the host.
The encoding never uses Protocol Buffers, because the field `locally_free` would split one channel into two keys.

**Rule HM-2: equal channels.** Two channels that are equal as `Par` values must have one key, so they name one collection.
The theorem `canon_respects_par_eq` (`Namespace.v:1035`) proves this rule for the key function of the model.

**Rule HM-3: order of lists.** The sorted form keeps the order of the elements of a list and of a tuple.
So `@[1, 2]` and `@[2, 1]` name two collections, as they name two channels of the host.

**Rule HM-4: names of a space.** The handle method `gensym` gives a new space name of 66 bytes, with a tail of 32 bytes.
The tail comes from the random state of the call, under the purpose byte `0x02` ([decision I-3](decision-register.md#i-3)).
The name routes to its space by rule R3, and its key follows rule HM-1.

### 2.2 Outer stores of later phases

Phase P5 adds the PathMap, Array, Vector, and HashSet outer stores.
The section of each store must state these items:

- the key function of the store, and the channels that share a key
- the names that the store makes, with their lengths and their random purposes
- the rules HM-1 to HM-4 that the store keeps, and the rules that it changes, with the reason for each change

## 3. Inner collections

An [inner collection](glossary.md#inner-collection) decides the order in which the entries of a collection interact.

### 3.0 Ordered collections

An ordered collection gives each entry an order, and the order decides which entries interact.
The queue of P4 is an ordered collection, and so are the stack, the cell, and the priority queue of P5.

**Rule OC-1: every operation bumps.** Every operation on an ordered collection consumes the collection counter with the value $`v`$ and produces it again with the value $`v + 1`$ ([decision 31](decision-register.md#decision-31)).
The operation takes the order $`v`$.
The rule covers a peek and a fire that changes no state too.
The counter consume must be a COMM without peeks, so that merge check 1 sees it.

**Rule OC-2: first operation.** The first operation on a collection finds no counter.
It consumes the space counter with the value $`m`$ and produces it again with $`m + 1`$.
It then creates the collection counter with the value 1, and it takes the order 0.

**Rule OC-3: fresh orders.** By rules OC-1 and OC-2, each operation on a collection has an order that no earlier operation on it had.
So the entries of a collection have distinct orders.

**Rule OC-4: limits.** When the counter that an operation consumes has the value $`2^{63} - 1`$, the operation must abort the deploy with `CounterOverflow`.
A collection with two counter records, or with a persistent counter record, is `Inconsistent`, and the operation must abort.

**Rule OC-5: races and chains.** Two branches that both operate on one collection consume one counter produce, so merge check 1 must report a race.
Two branches whose first operations open collections of one space race on the space counter in the same way.
Two deploys of one block that operate on one collection form one chain, so a rejection removes both.

**Rule OC-6: serializable merges.** When merge check 1 finds no race, the merged state must equal the state of a serial run of the two branches.

Without the bump of a read, two branches could make a [write skew](glossary.md#write-skew) [[BBG95](#ref-bbg95)].
Let branch A peek the queue `q1` and dequeue `q2`, while branch B peeks `q2` and dequeues `q1`.
The branches consume different records, so no check fires without the counters.
But no serial order of the branches gives both peeks, so the merge would accept a result that no serial run gives.
Rule OC-1 makes both branches race on the counters instead, and section [N3 of the design](design.md#n3-counters) shows the merge flow.

### 3.1 Queue

A queue is an ordered collection with [head-only matching](glossary.md#head-only-matching) on both sides ([decision 11](decision-register.md#decision-11)).
The rules use the state $`(R, W)`$ of one queue collection and the order $`o`$ of the operation.

**Rule QD-1: head.** The head of a side is its entry with the least order.
Two entries with one order compare by their bincode bytes, so the head is a function of the set of entries:

```math
\mathrm{head}(X) = \operatorname*{arg\,min}_{(o,\, e) \,\in\, X} \bigl(o,\ \mathrm{bincode}(e)\bigr)
```

**Rule QD-2: tail.** An incoming entry joins the tail of its own side.
So it meets the other side only when its own side is empty.
A re-issued persistent entry keeps its place instead, by rule QD-9.

**Rule QD-3: head-only matching.** A produce meets only the head waiter, and a consume meets only the head record.
A head that does not match blocks every entry behind it, also when an entry behind it would match.

**Rule QD-4: survivors of a fire.** After each fire, Table 6 decides whether the incoming entry is spent and whether the partner stays in the collection.

*Table 6. The incoming entry and the partner after one fire.*

| Operation | The incoming entry is spent when | The partner stays when |
|---|---|---|
| A produce of the datum $`d`$ fires the head waiter $`w`$ | $`d`$ is not persistent and $`w`$ does not peek | $`w`$ is persistent |
| A consume with the waiter $`w`$ fires with the head record $`d`$ | $`w`$ is not persistent | $`d`$ is persistent, or $`w`$ peeks |

A head record that stays because $`w`$ peeks stays by a [re-store](glossary.md#re-store) ([decision 45](decision-register.md#decision-45), Rule E).
The COMM of the peek removes the non-persistent record, as a host COMM removes a peeked datum.
The same transaction then stores the same record again, with the same bytes, envelope and order.
So the record keeps its place at the head, and the events equal those of the host program, whose reducer sends the peeked datum again.
The re-store registers the cost authority of the record, and it does not bump the counter.
A persistent record is never removed, so a peek of it stores nothing again.

**Rule QD-5: rounds.** After a fire, the operation takes one of three steps:

- When the incoming entry is spent, the operation ends.
- When the incoming entry survives and the partner stays, the pair is divergent. The operation commits, and it then [re-issues](glossary.md#re-issue) itself as a new [space intent](glossary.md#space-intent). A persistent operation keeps the order of its first round (rule QD-9).
- When the incoming entry survives and the partner left, the incoming entry meets the next head of the other side in the same transaction. After 64 fires in one transaction, the operation commits and re-issues itself ([decision I-21](decision-register.md#i-21)).

A produce that fires a peek waiter therefore offers its datum to the next head waiter ([decision 30](decision-register.md#decision-30)).
A persistent consume drains the records from the head, one record in each round.
An incoming entry that survives every fire and then meets no matching head stays in the collection, at the tail of its side.

**Reference algorithm.** An implementation must stage the same operations, in the same order, as this algorithm.
It writes $`\mathrm{fire}`$ for a staged COMM, $`\mathrm{store}`$ for a stored record, and $`\mathrm{park}`$ for a parked waiter.

```text
QUEUE-PRODUCE(R, W, d, o, kept):              ▷ the datum d with the order o; kept: QD-9
  if kept: at_head ← R = ∅ or o < ord(head(R))   ▷ QD-9: a re-entry at its place
  else:    at_head ← R = ∅                       ▷ QD-2: d joins the tail
  if not at_head: store (o, d), stop
  fired ← 0
  loop:
    if W = ∅: store (o, d), stop
    (o_w, w) ← head(W)                        ▷ QD-1
    if not match(w, d): store (o, d), stop    ▷ QD-3: the head blocks
    fire (d, w); fired ← fired + 1
    spent ← not persistent(d) and not peek(w)
    stays ← persistent(w)
    if not stays: W ← W \ {(o_w, w)}
    if spent: stop
    if stays: re-issue the produce, stop      ▷ a divergent pair (A10)
    if fired = 64: re-issue the produce, stop ▷ I-21
                                              ▷ a re-issue keeps o when d is persistent (QD-9)

QUEUE-CONSUME(R, W, w, o, kept):              ▷ the waiter w with the order o; kept: QD-9
  if kept: at_head ← W = ∅ or o < ord(head(W))
  else:    at_head ← W = ∅                    ▷ QD-2
  if not at_head: park (o, w), stop
  fired ← 0
  loop:
    if R = ∅: park (o, w), stop
    (o_d, d) ← head(R)
    if not match(w, d): park (o, w), stop
    fire (d, w); fired ← fired + 1
    if peek(w) and not persistent(d): store (o_d, d)   ▷ QD-4: the re-store (decision 45)
    spent ← not persistent(w)
    stays ← persistent(d) or peek(w)
    if not stays: R ← R \ {(o_d, d)}
    if spent: stop
    if stays: re-issue the consume, stop
    if fired = 64: re-issue the consume, stop ▷ a re-issue keeps o when w is persistent (QD-9)
```

Each operation stages its counter bump (rule OC-1) before its first step, and it commits once.
A re-issued operation is a new operation, so it bumps the counter again.
A re-issued persistent operation keeps the order of its first round.
Every other operation takes the value of its bump as its order (rule QD-9).

**Rule QD-6: invariant.** At the end of each completed deploy, no head pair is left that could fire:

```math
\mathrm{INV}(R, W) \iff R = \emptyset \;\lor\; W = \emptyset \;\lor\; \neg\, \mathrm{match}\bigl(\mathrm{head}(W),\ \mathrm{head}(R)\bigr)
```

Inside a deploy, a re-issued persistent entry can be in flight between two rounds.
Let $`F`$ be the set of these entries, each with its kept order and its side $`X_f`$.
After each committed transaction, a head pair can match only while an entry in flight precedes the head of its side:

```math
\mathrm{SHADOW}(R, W, F) \iff \mathrm{INV}(R, W) \;\lor\; \exists f \in F.\ \mathit{ord}(f) < \mathit{ord}\bigl(\mathrm{head}(X_f)\bigr)
```

A completed deploy has no entry in flight, because each re-issue runs in the same deploy, and a failed deploy is reverted.
With $`F = \emptyset`$, the rule $`\mathrm{SHADOW}`$ is $`\mathrm{INV}`$.

**Rule QD-7: first in, first out.** Records leave a queue in the order of their arrival, and waiters fire in the order of their arrival.
The arrival of a re-issued persistent entry is its first round, so the entry takes that place again when it re-enters (rule QD-9).

**Rule QD-8: no joins, params, or queries.** A join that reads a queue collection must abort with `OrderedJoinUnsupported` until P5.
A send with named params to a queue collection must abort with `UnknownParam`, because the discipline has no send param.
A receive with a query must abort with `QuerySpecUnsupported`.

**Rule QD-9: re-entry.** A re-issued persistent operation keeps the order $`o`$ of its first round ([decision 43](decision-register.md#decision-43)).
So its record, its identity, and its place are the same in every round.
Each of its rounds bumps the counter (rule OC-1).
It is at the head of its side exactly when the side is empty or $`o`$ is less than the order of the head.
Otherwise it is stored or parked at its place, and it fires nothing.

A stored entry with the order $`o`$ makes the state `Inconsistent`, and so does an order $`o`$ that is not below the counter.
A re-issued operation that is not persistent takes the value of its new bump as its order, as a new operation does.

**Example.** This program creates a queue space and uses the collection of `@"jobs"` in a `use` block.

```rholang
new hmq(`rho:space:hashmap:queue`) in { for (space <- hmq!?("default")) { use space { @"jobs"!(1) | for (@x <- @"jobs") { Nil } } } }
```

The send and the receive run in parallel, so either one can reach the collection first.
The deterministic scheduler picks one order, and play and replay pick the same order.
Table 7 shows both schedules, and both end with the same fire and an empty collection.

*Table 7. The two schedules of the example.*

| Schedule | First operation | Second operation | End state |
|---|---|---|---|
| The send first | Order 0: $`R = \emptyset`$ and $`W = \emptyset`$, so the record of the datum 1 is stored with the order 0. | Order 1: the head record matches, and the fire binds `x` to 1. | $`R = \emptyset`$, $`W = \emptyset`$, counter 2 |
| The receive first | Order 0: $`R = \emptyset`$, so the waiter is parked with the order 0. | Order 1: the head waiter matches, and the fire binds `x` to 1. | $`R = \emptyset`$, $`W = \emptyset`$, counter 2 |

Figure 1 shows the states of one queue collection and the transitions between them.

![States of one queue collection as a state machine. A collection starts unopened. Its first operation consumes the space counter S(sid) and produces it again, opens S(sid, k) = 1, and takes the order 0. The open collection is in one of four states: Empty with R and W empty, Records queued with R not empty, Waiters queued with W not empty, and Blocked with both sides not empty, where the head waiter does not match the head record and no fresh operation leaves the state, while a re-entry ahead of a head can fire. A send without a waiter stores the record, and a receive without a record parks the waiter. A receive that fires with the head record removes it, or keeps it: a peek stores it again in the same transaction (decision 45), and a persistent record stays. A receive that does not match the head record parks the waiter and blocks. A persistent receive drains every record and then parks. A send that fires the head waiter removes it unless it is persistent. A datum that survives every fire, after a peek waiter or as a send !!, is stored when no waiter matches. A note gives the rules: every operation bumps S(sid, k) from v to v + 1 and takes the order v, a re-issued persistent operation keeps the order of its first round and re-enters at its place, the head of a side is the least (ord, bincode), the re-offer, the re-store of a peeked record, the drain, the re-issue of a divergent pair after one round, the re-issue of any chain after 64 fires, and the invariant INV at the end of every deploy.](diagrams/05-head-only-queue.svg)

*Figure 1. Source: [diagrams/05-head-only-queue.puml](diagrams/05-head-only-queue.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

### 3.2 Disciplines of later phases

Phases P5 and P6 add the other disciplines.
The section of each discipline must state these items, which the trait `Discipline` of the design also names:

- the head rule of each side, and whether an incoming entry reaches the head
- whether the discipline uses the counters of rules OC-1 to OC-6
- whether a receive can join a collection of the discipline with other channels
- the named send params, the query, and their ranges
- the rules QD-1 to QD-9 that the discipline keeps, and the rules that it replaces

## 4. Qualifiers

A [qualifier](glossary.md#qualifier) sets the persistence and the concurrency of a space.

### 4.1 Default

**Rule QF-1: the default qualifier.** A constructor without a qualifier creates a space with the qualifier `"default"`.
The constructor `"default"` has the same effect.

**Rule QF-2: durable.** A default space is durable.
Its ids use the uppercase [KIND](glossary.md#kind) bytes, and its records are ordinary channels of the RSpace of the node.
A checkpoint and a restart must keep its config record, its counters, its records, and its waiters.

**Rule QF-3: concurrent.** A default space accepts operations from parallel branches of one deploy and from many deploys.
The scheduler orders the operations of one space, and it runs the operations of different spaces in parallel.

**Rule QF-4: families.** Every outer store takes the qualifier `"default"`, except the HashSet store, which takes only `"seq"`.
When a HashSet family is available, a constructor with another qualifier must abort with `BadConstructorArgument`.

### 4.2 Qualifiers of later phases

Phase P7 adds the qualifiers `"temp"` and `"seq"`.
Until then, a constructor with either qualifier must abort with `QualifierUnavailable`.
The section of P7 must state the durability, the sweep of the state at the end of a deploy, and the rule of sequential access.

## 5. Theories

Phase P7 adds the theories of the FIPS, which a constructor names with `free Th(args)`.
Until then, a constructor with a theory must abort with `TheoryUnavailable`, and the config map holds `Nil` at the key `"theory"`.
The section of P7 must state the admission of a datum under a theory and its charge.

## 6. Constructs that fail closed

P4 rejects each construct that a later phase adds with a deterministic code ([decision 34](decision-register.md#decision-34), item A9).
The [syntax contract](syntax-contract.md#meaning-at-run-time) lists each construct with its code and its phase.
Phase P8 replaces the code `AgentSpacesUnavailable` with the rules of spaces that user agents implement.

## 7. Conformance

Table 8 maps each rule to the formal evidence and to the tests that check it.
The [verification dossier](verification.md#11-phase-p4-the-hashmapqueue-slice) gives the commands and the results.
A Rocq theorem shows the file and the line of its statement, and a TLA+ invariant shows its id in the invariant map.

*Table 8. The rules of this specification and their evidence.*

| Rule | Formal evidence | Tests |
|---|---|---|
| SM-1 | `sid_derivation_deterministic` (`SpaceFactory.v:650`), `space_already_exists_is_deterministic_abort` (`SpaceFactory.v:794`) | `prop_sid_derivation_deterministic`, `space_already_exists_aborts` |
| SM-2 | `counter_and_config_roundtrip` (`RecordEnvelope.v:994`), the TLA+ invariant `HandleUnique` (U5) | `prop_counter_and_config_records_roundtrip`, `space_meta_channels_config_record` |
| SM-3 | The TLA+ invariant `DanglingIsDeterministicError` (U6) | `reified_handle_dangling_aborts`, `space_queue_dangling_name_aborts` |
| SM-4, SM-5 | `storage_id_injective` (`Namespace.v:727`), `space_and_collection_counters_distinct` (`Namespace.v:784`) | `space_meta_channels_counters`, `space_queue_events_namespaced` |
| SM-8 | The P1 headlines of the substrate, which section [4 of the dossier](verification.md#4-rocq-models) lists | `space_queue_observer_parity`, `reified_p4_smoke_hashmap_queue_play_and_replay` |
| SM-9 | `queue_plan_permutation_invariant` (`HeadOnlyDisciplines.v:1552`) | `prop_queue_plan_permutation_invariant`, `space_queue_ord_survives_restart` |
| SM-10 | `route_rules` (`SpaceRefs.v:624`), `space_name_route_ignores_ctx` (`SpaceRefs.v:645`), `ordinary_channels_route_default` (`SpaceRefs.v:710`) | `prop_route_rules`, `reducer_space_routing_ordinary_unchanged` |
| HM-1, HM-2 | `canon_respects_par_eq` (`Namespace.v:1035`) | `prop_canonical_inner_bytes_respects_par_eq` |
| HM-4 | `name_id_routes_to_sid` (`Namespace.v:995`) | `prop_name_id_routes_to_sid`, `space_handle_methods_gensym` |
| OC-1, OC-3 | `queue_ord_fresh` (`HeadOnlyDisciplines.v:1046`) | `prop_queue_ord_fresh`, `space_queue_counter_bump_shape` |
| OC-2 | `first_ops_race_on_space_counter` (`CounterConflict.v:772`) | `prop_first_ops_race_on_space_counter`, `space_counter_first_operation_bumps_the_space_counter` |
| OC-4 | None | `space_counter_rejects_corrupt_counters` |
| OC-5 | `ordered_writes_race_on_counter` (`CounterConflict.v:743`), `ordered_reads_race_on_counter` (`CounterConflict.v:757`), `same_block_ops_chain` (`CounterConflict.v:946`) | `space_queue_counter_race_conflict_enq_deq`, `space_queue_counter_race_conflict_same_block_one_chain` |
| OC-6 | `merge_without_race_is_serializable` (`CounterConflict.v:1322`), the TLA+ invariant `NoMergedOrderFabrication` (U4) | `prop_nonracing_queue_branches_serializable`, `space_queue_counter_race_conflict_unrelated_queues_merge` |
| QD-1 | `queue_head_is_min_ord` (`HeadOnlyDisciplines.v:530`) | `prop_queue_candidates_head_only` |
| QD-2, QD-3 | `queue_head_blocks` (`HeadOnlyDisciplines.v:1218`), the TLA+ invariant `HeadBlocks` (U2) | `space_queue_head_only_data_head_blocks`, `space_queue_head_only_waiter_head_blocks` |
| QD-4, QD-5 | `queue_loop_bounded_or_reissues` (`HeadOnlyDisciplines.v:1380`) | `space_queue_reissue_persistent_send`, `space_queue_reissue_peek_produce`, `space_queue_divergent_pair_exhausts_phlo` |
| QD-6 | `queue_produce_preserves_inv` (`HeadOnlyDisciplines.v:928`), `queue_consume_preserves_inv` (`HeadOnlyDisciplines.v:948`), the TLA+ invariants `QuiescentNoPendingMatch` (U1) and `ShadowedNoPendingMatch` (U9), `queue_reentry_preserves_shadow_inv` (`HeadOnlyDisciplines.v:2712`), `queue_quiescent_inv` (`HeadOnlyDisciplines.v:2781`) | `prop_queue_produce_preserves_invariant`, `prop_queue_consume_preserves_invariant`, `prop_queue_shadow_invariant_with_inflight` |
| QD-7 | `queue_fifo` (`HeadOnlyDisciplines.v:1135`), `queue_fifo_with_reentries` (`HeadOnlyDisciplines.v:2946`), the TLA+ invariants `FifoOrder` (U3) and `LeaveOrder` (U12) | `space_queue_head_only_fifo_delivery`, `prop_queue_fifo_with_reentries` |
| QD-8 | None | `space_queue_join_rejected`, `space_queue_send_params_rejected`, `space_queue_query_rejected` |
| QD-9 | `queue_reissue_keeps_ord` (`HeadOnlyDisciplines.v:2373`), `queue_head_blocks_reentry` (`HeadOnlyDisciplines.v:3021`), `persistent_reentry_never_spent` (`HeadOnlyDisciplines.v:2342`), the TLA+ invariants `ReissueKeepsOrd` (U10), `IntroducedOnce` (U11) and `FifoOrder` (U3) | `space_queue_reissue_reenters_ahead_of_younger_records`, `prop_queue_reissue_keeps_first_round_ord`, `prop_queue_reentry_head_blocks` |
| QF-1, QF-4 | `qualifier_matrix` (`SpaceFactory.v:372`) | `prop_qualifier_matrix`, `space_urn_factory_qualifier_support_matrix` |
| QF-2 | The TLA+ invariant `DurableSurvivesRestart` (U7) | `prop_handle_survives_checkpoint_reset`, `reified_handle_persisted_restart_replay` |
| QF-3 | The TLA+ invariant `HandleCallsMergeLikeContracts` (U8) | `space_intents_for_two_sids_in_one_frontier_run_in_separate_tasks`, `reified_handle_cross_block_merge_no_conflict` |

No model states the rules OC-4 and QD-8, because they are checks of limits and of input.
Their tests show each abort.

## References

<a id="ref-bbg95"></a>[BBG95] H. Berenson, P. Bernstein, J. Gray, J. Melton, E. O'Neil, and P. O'Neil, "A critique of ANSI SQL isolation levels," in *Proceedings of the 1995 ACM SIGMOD International Conference on Management of Data*, ACM, 1995, pp. 1–10. [doi:10.1145/223784.223785](https://doi.org/10.1145/223784.223785)
