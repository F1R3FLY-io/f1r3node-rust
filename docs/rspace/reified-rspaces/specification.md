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
- [8. Native execution](#8-native-execution)
- [9. System processes of the VectorDB family](#9-system-processes-of-the-vectordb-family)
- [10. The ChromaDB mirror](#10-the-chromadb-mirror)
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
| P5 | [2.2](#22-array) to [2.6](#26-the-pathmap-store) | The Array and Vector outer stores with their allocator, the PathMap paths, and the [PathMap store](#26-the-pathmap-store) |
| P5 | [3.2](#32-bag) to [3.8](#38-joins) | The bag, set, stack, cell, and priority queue disciplines, the receive queries, the query of a priority queue, and the [joins of one space](#38-joins) ([decision 29](decision-register.md#decision-29)) |
| P6 | [3.9](#39-vectordb), [9](#9-system-processes-of-the-vectordb-family) and [10](#10-the-chromadb-mirror) | The VectorDB discipline, its query, and its bounds, the system processes of the family, and the ChromaDB mirror |
| P7 | [2.7](#27-hashset), [4.2](#42-temp), [4.3](#43-seq) and [5](#5-theories) | The HashSet store, the `temp` and `seq` qualifiers, the sweep of their state, the dangling ids of ended deploys, and the theories |
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
| `"theory"` | `Nil`, or one `GByteArray` with the canonical bytes of the identity of the theory of the space (Rule TH-2) | `Nil` |

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
The waiter of a receive with one bind has one bind.
A join waiter has one bind for each channel of its receive, and it waits on all its channels at once ([section 3.8](#38-joins)).

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
| R4 | A name of a user agent | The user agent. Since P8 step 2 a send becomes the call `A!(sink, "produce", raw, [d…])` on the agent. Since P8 step 3 a receive whose channels all route to the agent becomes its matcher receives, its body receive, and the call `A!(sink, "consume", pairs, arrow, b)` on the agent |
| R5 | Any other channel, such as `@"jobs"` | The collection $`\mathrm{key}(c)`$ of the space of the use context, or the default space without a use context |
| R6 | A channel that routes to a collection of a native space | The operation uses the data channel $`D(\mathit{sid}, k)`$ of the collection |
| R7 | The reply channel of a system process | The rules R1 to R5 without a use context. Since P8 step 2, the bundle with both flags of a name of a user agent also routes to its agent (R7A) |

### 1.5 Families

A family pairs one outer store with one inner collection, for example `hashmap:queue`.
Its factory creates the spaces of the family, and its agent runs their operations.

**Rule FA-1: registration.** A family is registered exactly when it has a factory row and the family dispatch has its arm (`rholang/src/rust/interpreter/spaces/factory/urn.rs:203`, `rholang/src/rust/interpreter/spaces/factory/urn.rs:231-237`).
A registered family with the index $`i`$ binds its URN to `bundle+{[0xE0, i]}`, and it installs its factory with the body reference $`200 + i`$.
An unregistered family has no URN binding, so a program that names it fails before any space work.

**Rule FA-2: dispatch.** The family agent reads the config of the space once and runs the agent of the family that the config names (`rholang/src/rust/interpreter/spaces/families.rs:540-549`).
The dispatch tries the arms in a fixed order: `hashmap:queue`, ascending family index, the PathMap families, the VectorDB families, then the HashSet families (`rholang/src/rust/interpreter/spaces/families.rs:258-291`).
Each arm that it tries reserves one comparison of the names, so a queue operation keeps the host work of P4.
New arms come after the old ones, so the dispatch of an earlier family reserves the comparisons that it reserved before.
A config that names no wired family fails with `FamilyUnavailable`.

**Rule FA-3: aliases.** The FIPS alias `rho:space:HashMapBagSpace` names the channel of `hashmap:bag` (`rholang/src/rust/interpreter/reified/system_channels.rs:310-314`).
The FIPS alias `rho:space:PathMapSpace` names the channel of `pathmap:bag` (`rholang/src/rust/interpreter/reified/system_channels.rs:318-322`).
An alias is registered exactly when its family is registered, and both URNs bind the same capability.
`getSpaceAgent` replies that capability for every space of the family ([decision 33](decision-register.md#decision-33)).

**Rule FA-4: constructor schemas.** Table 1a lists the named params that the factory of each family accepts and requires (`rholang/src/rust/interpreter/spaces/factory/defaults.rs:107-156`).

*Table 1a. The constructor schemas of the families of P5 and P7.*

| Families | Params | Required |
|---|---|---|
| `hashmap`, `pathmap`, `vector`, `hashset`: bag, queue, stack, set, cell | none | none |
| `hashmap`, `pathmap`, `vector`, `hashset`: `priorityqueue` | `priorities` | none |
| `array`: bag, queue, stack, set, cell | `cyclic`, `size` | `size` |
| `array`: `priorityqueue` | `cyclic`, `priorities`, `size` | `size` |

The VectorDB families of P6 have the schema of section [3.9](#39-vectordb).
The HashSet families take the params of their discipline only (rule HS-7).

**Rule FA-5: param values.** `size` is a `GInt` in $`[1, 2^{63}-1]`$, and `cyclic` is a `GBool` (F7).
`priorities` is a `GInt` of at least 1 ([decision I-10](decision-register.md#i-10)).
The bit sets of a value do not count, and a value with the connective flag fails (`rholang/src/rust/interpreter/spaces/factory/defaults.rs:173-188`).

**Rule FA-6: gate order.** The gates of a constructor run in this order, and every gate runs before the space exists (`rholang/src/rust/interpreter/spaces/factory/constructor.rs:87-178`):

1. the schema (`FamilyUnavailable`)
2. the parse (`BadConstructorArgument`)
3. the qualifier (`BadConstructorArgument`, `QualifierUnavailable`)
4. the theory (`BadConstructorArgument`, `UnknownTheory`, Rule TH-4)
5. positional params (`PositionalParamsUnsupported`)
6. unknown keys in bytewise order (`UnknownParam` with the nearest key within two edits)
7. missing required keys (`MissingParam`)
8. values in bytewise key order (`InvalidParam`)

**Rule FA-7: stored params.** The config holds each named param in the canonical form of its rule, and an absent optional param stays absent (`rholang/src/rust/interpreter/spaces/factory/constructor.rs:265-279`).
The readers apply the defaults: an Array space without `cyclic` is fixed, and a priority queue without `priorities` has no bound on its levels.

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
The theorem `canon_respects_par_eq` (`Namespace.v:1043`) proves this rule for the key function of the model.

**Rule HM-3: order of lists.** The sorted form keeps the order of the elements of a list and of a tuple.
So `@[1, 2]` and `@[2, 1]` name two collections, as they name two channels of the host.

**Rule HM-4: names of a space.** The handle method `gensym` gives a new space name of 66 bytes, with a tail of 32 bytes.
The tail comes from the random state of the call, under the purpose byte `0x02` ([decision I-3](decision-register.md#i-3)).
The name routes to its space by rule R3, and its key follows rule HM-1.

### 2.2 Array

An Array space has a fixed number of names.
Its config holds the param `size` and the optional param `cyclic`, as rules FA-5 and FA-7 state.

**Rule AR-1: index names.** A name of an Array space is the index name $`N(\mathit{sid} \Vert \mathrm{u64\_be}(i))`$ of 42 bytes, with the index $`i`$ as its tail.
Only the allocator makes index names (rule AL-1), so a program never forges one.

**Rule AR-2: collection key.** An Array space keys, names and counts its collections as the HashMap store does, so rules HM-1 to HM-3 hold (`rholang/src/rust/interpreter/spaces/outer/array.rs:130-197`).
The data channel of the index name $`c`$ is $`D(\mathit{sid}, \mathrm{key}(c))`$.

**Rule AR-3: accepted channels.** A collection operation on an Array space accepts the channel $`c`$ only when $`c`$ is an index name of the space with $`i < \mathit{size}`$.
Every other channel fails with `NotAnIndexName` before the operation stages anything (F8, `rholang/src/rust/interpreter/spaces/outer/array.rs:142-152`).
This includes a forged channel, a name with a random tail and a name of another space.

**Rule AR-4: fixed and cyclic allocation.** A fixed Array space allocates the indices $`0, 1, \ldots, \mathit{size}-1`$ once each, in order.
A cyclic Array space allocates them in order modulo the size, so it gives each name again after $`\mathit{size}`$ allocations.
Two holders of one name then share its collection.

**Rule AR-5: no storage before use.** A collection exists from its first operation. The space preallocates nothing.

### 2.3 Vector

**Rule VE-1: index names.** A Vector space names its collections by index names, as rule AR-1 states. It has no size, and it takes no `size` param.

**Rule VE-2: collection key.** Rule AR-2 holds for a Vector space (`rholang/src/rust/interpreter/spaces/outer/vector.rs:34-98`).

**Rule VE-3: accepted channels.** A collection operation on a Vector space accepts every index name of the space and no other channel (`NotAnIndexName`, F8, `rholang/src/rust/interpreter/spaces/outer/vector.rs:46-54`).

**Rule VE-4: allocation.** A Vector space allocates the indices $`0, 1, 2, \ldots`$ once each, in order, up to $`2^{63}-1`$ names.

### 2.4 The allocator of Array and Vector spaces

**Rule AL-1: the allocator record.** The record $`A(\mathit{sid})`$ holds the next index $`n`$ in the counter format (`rholang/src/rust/interpreter/spaces/allocator.rs:259-277`).
The creation of an Array or a Vector space stages $`A(\mathit{sid}) = 0`$ after the space counter.
The record takes the random state $`\mathrm{Allocator}.\mathrm{derive}(r).\mathrm{split\_byte}(1)`$ of the factory call $`r`$ and the cost authority of the caller.

**Rule AL-2: allocation.** An allocation of $`k`$ names is one job over the lock keys $`[C(\mathit{sid}), A(\mathit{sid})]`$ (`rholang/src/rust/interpreter/spaces/allocator.rs:283-296`).
It plans first, and then it takes $`A(\mathit{sid}) = n`$ and puts $`A(\mathit{sid}) = \mathit{next}`$ (`rholang/src/rust/interpreter/spaces/allocator.rs:109-162`):

| Store | Indices | Next value | Failure |
|---|---|---|---|
| Fixed Array of size $`s`$ | $`n, \ldots, n+k-1`$ | $`n+k`$ | `OutOfNames` when $`n + k > s`$ |
| Cyclic Array of size $`s`$ | $`(n+i) \bmod s`$ for $`0 \le i < k`$ | $`(n+k) \bmod s`$ | none |
| Vector | $`n, \ldots, n+k-1`$ | $`n+k`$ | `OutOfNames` when $`n + k > 2^{63}-1`$ |

**Rule AL-3: no partial allocation.** An allocation that fails stages nothing, so it allocates no name and leaves $`A(\mathit{sid})`$ as it was.
An allocation that succeeds stages exactly one take and one put, also for $`k = 0`$.

**Rule AL-4: identities.** The take uses the random state $`\mathrm{Allocator}.\mathrm{derive}(r_{\mathrm{job}}).\mathrm{split\_byte}(0)`$, and the put uses $`\mathrm{split\_byte}(1)`$.
So the identities of each job are new, also when a cyclic allocator takes a value again.

**Rule AL-5: races.** Two branches that allocate from one space consume one produce of $`A(\mathit{sid})`$.
So merge check #1 sees their race, and a merge never hands one index to both branches (`NoDuplicateIndex`).
Two `new` blocks inside `use` of one Array or Vector space race in the same way (`rholang/tests/reified_merge_p5.rs:930-991`).

**Rule AL-6: other stores.** A HashMap or PathMap space has no allocator.
Its allocation job answers `Baseline`: it reads the config, commits nothing, and charges no phlo.
The reducer then draws the names as it does for every `new`.

**Rule AL-7: metering and native execution.** An allocation reserves each read and copy before it makes it ([decision 42](decision-register.md#decision-42)).
This covers the names of its lock keys, the config and the shape of an Array space.
It also covers the copy and the decode of $`A(\mathit{sid})`$, each comparison, and the buffer of the indices.
In native execution, an allocation job runs as every space job does ([section 8](#8-native-execution), [decision 47](decision-register.md#decision-47)).

**Rule AL-8: authority.** An allocation job takes the cost authority of the process that asks for the names (`rholang/src/rust/interpreter/reified/reducer_hooks.rs:975-1002`).
That process evaluates the `new`, or it calls the handle method `gensym`.
So each allocation is paid by its caller, and the allocator record that the job puts carries that authority (P4 design B.7).

### 2.5 PathMap paths and re-rooting

The PathMap store of wave W3 uses this pure layer of paths, keys and re-rooting.

**Rule PP-1: paths.** A channel of a PathMap space names a path, a list of path elements (`rholang/src/rust/interpreter/spaces/pathmap/path_codec.rs:281-286`).
When the channel $`c`$ is a plain list, its path holds the elements of $`c`$. For every other channel, the path is $`[c]`$.
A plain list is one `EList` with no remainder, empty bit sets and no other part.
So `@c` and `@[c]` name one path, and a fresh name roots a private subtree.

**Rule PP-2: keys.** The path key of a path $`p = [e_1, \ldots, e_n]`$ and the prefix key of a prefix $`q = [e_1, \ldots, e_k]`$ are these byte strings (`rholang/src/rust/interpreter/spaces/pathmap/path_codec.rs:343-347`):

```math
\mathrm{path\_key}(p) = \mathrm{seg}(e_1) \,\Vert\, \cdots \,\Vert\, \mathrm{seg}(e_n) \,\Vert\, \mathtt{0x00}
\qquad
\mathrm{prefix\_key}(q) = \mathrm{seg}(e_1) \,\Vert\, \cdots \,\Vert\, \mathrm{seg}(e_k)
```

The segments are a prefix code, and no segment starts with the terminator $`\mathtt{0x00}`$.
So the prefix order of paths is the byte-prefix order of their keys (`prefix_key_iff_prefix`):

```math
q \preceq p \iff \exists z.\; \mathrm{path\_key}(p) = \mathrm{prefix\_key}(q) \mathbin{+\!\!+} z
```

The terminator is below every tag, so the exact path is the first candidate of a receive (`candidate_order_total`).

**Rule PP-3: depth.** A channel path or a prefix with more than 64 elements fails with `PathTooDeep` (F9).
A payload entry is not a channel, so the key of an entry has no bound.

**Rule PP-4: segments.** The segment codec is byte-identical to EPM1 for every element that both codecs accept (`rholang/src/rust/interpreter/spaces/pathmap/path_codec.rs:369-371`).
An element that holds an `EPathMap` or an `EZipper` at any depth fails with `UnsupportedPathElement`.
An element outside the structural grammar takes the escape arm, which holds the Protocol Buffers bytes of the sorted `Par`.

**Rule PP-5: re-rooting.** A send on $`p`$ is a send on every prefix $`q \preceq p`$.
A receive on $`q`$ gets each argument re-rooted under the suffix $`s = p \setminus q`$ (`rholang/src/rust/interpreter/spaces/pathmap/reroot.rs:51-75`):

```math
R(\varepsilon, v) = v \qquad
R(s, \{\!|\,|\!\}) = \{\!|\, s \,|\!\} \qquad
R(s, m) = \{\!|\, s \mathbin{+\!\!+} \mathrm{elems}(e) : e \in m \,|\!\} \qquad
R(s, v) = [s_1, \ldots, s_k, v]
```

The last three rules apply only when $`s \neq \varepsilon`$.
The third rule applies to a PathMap with entries, and the fourth rule to every value that is not a PathMap.
The FIPS example holds: `@[0,1,2]!({|"hi"|})` on `@[0,1]` binds `{|[2, "hi"]|}`, and `@[0,1,2]!(42)` on `@[0,1]` binds `[2, 42]`.

**Rule PP-6: the adapter.** The adapter turns an `EPathMap` into its logical map and back (`rholang/src/rust/interpreter/spaces/pathmap/adapter.rs:293-337`).
The decode orders the paths by entry key and keeps the first entry of each key.
The encode writes each path in its canonical entry form, so $`\mathrm{decode}(\mathrm{encode}(m)) = m`$.

### 2.6 The PathMap store

The PathMap store keeps each datum on the data channel of its path ([decision 9.3](decision-register.md#decision-9-3)).
It also keeps a [path marker](glossary.md#path-marker) of the datum on the [index channel](glossary.md#index-channel) of each prefix of the path.
The [PathMap delivery](pathmap-prefix-delivery.md) explains the model, the plans and their proofs.
Phase 1 of wave W3 implements the store, the markers and the delivery plans.
Phase 2 runs the plans on the join engine and registers the PathMap families ([decision 51](decision-register.md#decision-51)).

**Rule PS-1: channels.** The data channel $`D(\mathit{sid}, \mathrm{path\_key}(p))`$ holds the records of the path $`p`$.
The index channel $`I(\mathit{sid}, \mathrm{prefix\_key}(q))`$ holds the markers and the waiters of the prefix $`q`$.
The [token](glossary.md#token) $`S(\mathit{sid})`$ is the counter of every path, and the store has no allocator (`rholang/src/rust/interpreter/spaces/outer/pathmap.rs:85-96`, `218-220`).

**Rule PS-2: keys and names.** The store keeps the rules HM-2 to HM-4.
Its collection key is the path key of rule PP-2 instead of the key of rule HM-1.
Its key check applies the rules PP-3 and PP-4 before any staging (`rholang/src/rust/interpreter/spaces/outer/pathmap.rs:201-210`).
A name that the space makes is a fresh name, so it roots a private subtree (rule PP-1).

**Rule PS-3: markers.** A stored datum $`d`$ on $`p`$ has exactly one marker on $`I(q)`$ for each prefix $`q \preceq p`$, and no other marker exists.
The marker holds the pars `[EList[tag_I, GByteArray(src(d))], EList(p∖q)]`, with the tag `[0xE0, 0x52, 0x49, 0x01]` and the source hash $`\mathrm{src}(d)`$ of $`d`$.
Its random state derives from the random state of $`d`$ under the purpose `PathMarker` and the depth of $`q`$ (`rholang/src/rust/interpreter/spaces/pathmap/markers.rs:137-165`).
Its persistence equals that of $`d`$, and it holds the cost authority of the record of $`d`$ and no cost stack (`marker_authority`, `rholang/src/rust/interpreter/spaces/pathmap/markers.rs:130-132`).

**Rule PS-4: vehicle.** The [vehicle](glossary.md#vehicle) of $`d`$ on $`q`$ is the marker pars of $`d`$ on $`q`$, followed by the payload of $`d`$.
It has the random state, the authority and the cost stack of $`d`$, and no store keeps it (`rholang/src/rust/interpreter/spaces/pathmap/markers.rs:197-213`, [decision 40](decision-register.md#decision-40), Q3).

**Rule PS-5: receive.** The candidates of a receive on $`q`$ are the datums under $`q`$, in path-key order, with the exact path first.
Within one path they follow the order of the discipline.
The receive takes the first candidate whose re-rooted payload its pattern binds, and it checks its guard once on that candidate ([decision 37](decision-register.md#decision-37)).
A refusal parks the receive on $`I(q)`$. The delivery COMM is on $`I(q)`$, over the marker of the datum on $`q`$ (`rholang/src/rust/interpreter/spaces/pathmap/delivery.rs:175-249`).

**Rule PS-6: send.** A send on $`p`$ offers its vehicle to the waiters of $`I(p)`$, and then to the waiters of each shorter prefix, up to $`I(\varepsilon)`$.
The first waiter whose pattern binds the re-rooted payload and whose guard admits it fires, and a refusal moves on.
Without a fire, the send stores its record on $`D(p)`$ and then its markers in depth order (`rholang/src/rust/interpreter/spaces/pathmap/delivery.rs:389-431`, `460-487`).

**Rule PS-7: retire.** A removal of a datum stages a single-channel `K_retire` on $`D(p)`$.
It then stages one on each marker of the datum, except the marker of the delivery COMM (`rholang/src/rust/interpreter/spaces/pathmap/delivery.rs:542-551`).

**Rule PS-8: peeks.** A peek COMM removes only the marker of the datum on the prefix of the receive ([decision 45](decision-register.md#decision-45), Rule E).
The second send then offers the datum to the waiters of its path and its prefixes, the most specific first.
A fire of a waiter that does not peek spends the datum by rule PS-7.
Without such a fire, the second send restores the marker with its identity.

At the fire cap, the transaction stores the marker again before its commit (open item O-2).
The settle job then carries only the offer of the datum.

**Rule PS-9: token.** Every operation of an ordered discipline bumps the token, and a re-issued persistent operation keeps its first-round order ([decision 31](decision-register.md#decision-31), [decision 43](decision-register.md#decision-43)).
On a bag or a set, every operation that changes the state bumps the token, and every order is 0.
A peek of a bag or a set bumps the token only when its second send changes the state ([decision 51](decision-register.md#decision-51)).

**Rule PS-10: merges.** A new datum on $`p`$ and a new waiter on a prefix $`q \preceq p`$ meet on $`I(q)`$, so merge check #2 sees the contact.
Two branches that change one PathMap space race on its token under merge check #1 (`pathmap_token_race`, headline 15 of `CounterConflict.v`).

### 2.7 HashSet

A HashSet space is the default space restricted to empty messages (FIPS 213-215, [decision 73](decision-register.md#decision-73)).
Each of its collections holds [units](glossary.md#unit): records whose payload is empty.
The HashSet store implements this restriction (`rholang/src/rust/interpreter/spaces/outer/hashset.rs:38-76`).

**Rule HS-1: keys and names.** The HashSet store keeps the rules HM-1 to HM-4 of the HashMap store.
It keys a channel, makes the data and counter channels, and draws the names of `gensym` as the HashMap store does.
It changes none of these rules.

**Rule HS-2: sends.** A send to a HashSet collection carries the empty payload: `x!()`, or the handle call `S!?("produce", c, [])`.
A send with any value, `x!(Nil)` included, must abort the deploy with `PayloadNotEmpty` before any staging.
The named params of the discipline still apply, for example `priority` on `hashset:priorityqueue`.

**Rule HS-3: receives.** Every bind on a HashSet collection has the empty pattern list: `for ( <- x)`.
A pattern, a binder or a remainder must abort the deploy with `PatternNotEmpty` before any staging.
The arrows `<-`, `<=` and `<<-` keep their meaning.
A bind has no variable, so a `where` guard can name only the variables around the receive.

**Rule HS-4: joins.** A join of empty binds on collections of one HashSet space runs in the join engine of section [3.8](#38-joins).
A join across spaces must abort with `CrossSpaceJoin`.

**Rule HS-5: inner collections.** An empty pattern list matches exactly the empty payload, so every waiter matches every unit.
The discipline then orders the units and the waiters as in every other store:

- `set` keeps one unit class for each channel: two adds and one take leave the collection empty.
- `bag` counts the units, `cell` holds one unit (`CellOccupied`), and `queue`, `stack` and `priorityqueue` take their head unit.

**Rule HS-6: qualifier.** Every HashSet space has the qualifier `"seq"` (rule [QF-4](#41-default)), so the Seq rules of section [4.3](#43-seq) apply.
A receive of an absent unit waits for the rest of the deploy.
So a process never observes that a unit is absent (SQ-6 (d) of the P7 design).

**Rule HS-7: families.** The six HashSet families have the indices 26 to 31 and the body references 226 to 231.
Their factories take the params of their discipline only (Table 1a), after the qualifier `"seq"`.

The checks of HS-2 and HS-3 are hooks of the outer store.
They run after the key check and before the params, in the driver and in the join engine (`rholang/src/rust/interpreter/spaces/generic.rs:2586-2591`, `rholang/src/rust/interpreter/spaces/join.rs:412-471`).
Each check reserves one operation on the meter of the job before it reads.
Headline `hashset_unit_rules` of `HashMapStore.v` proves the unit rules.

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
This holds for every family with a counter: the queue, the stack and the cell, also in Array and Vector spaces (`shared_collection_race`, `CounterConflict.v:889`).

**Rule OC-6: serializable merges.** When merge check 1 finds no race, the merged state must equal the state of a serial run of the two branches.

Without the bump of a read, two branches could make a [write skew](glossary.md#write-skew) [[BBG95](#ref-bbg95)].
Let branch A peek the queue `q1` and dequeue `q2`, while branch B peeks `q2` and dequeues `q1`.
The branches consume different records, so no check fires without the counters.
But no serial order of the branches gives both peeks, so the merge would accept a result that no serial run gives.
Rule OC-1 makes both branches race on the counters instead, and section [N3 of the design](design.md#n3-counters) shows the merge flow.

Without the bump of a removal, two branches could merge [crossed stacks](glossary.md#crossed-stacks).
Let the stacks `a` and `b` hold 10 and 20.
Branch A pushes 1 onto `a` and pops `b`, and branch B pushes 2 onto `b` and pops `a`.
The pushes bump different counters and the pops take different records, so no check fires when a pop bumps no counter.

In the merge A pops 20 and B pops 10.
But after A, B pops 1, and after B, A pops 2, so no serial order gives the merged pops.
Rule OC-1 makes both branches race on both counters instead (`crossed_stacks_without_removal_counters_fabricate`, `CounterConflict.v:2578`).

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

**Rule QD-8: joins, params, and queries.** A join that reads a queue collection follows [section 3.8](#38-joins), since P5 wave W3 retired `OrderedJoinUnsupported` ([decision 29](decision-register.md#decision-29)).
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

![States of one queue collection as a state machine. A collection starts unopened. Its first operation consumes the space counter S(sid) and produces it again, opens S(sid, k) = 1, and takes the order 0. The open collection is in one of four states: Empty with R and W empty, Records queued with R not empty, Waiters queued with W not empty, and Blocked with both sides not empty, where the head waiter does not match the head record and no fresh operation leaves the state, while a re-entry ahead of a head can fire. A send without a waiter stores the record, and a receive without a record parks the waiter. A receive that fires with the head record removes it, or keeps it: a peek stores it again in the same transaction (decision 45), and a persistent record stays. A receive that does not match the head record parks the waiter and blocks. A persistent receive drains every record and then parks. A send that fires the head waiter removes it unless it is persistent. A datum that survives every fire, after a peek waiter or as a send !!, is stored when no waiter matches. A note gives the rules: every operation bumps S(sid, k) from v to v + 1 and takes the order v, a re-issued persistent operation keeps the order of its first round and re-enters at its place, the head of a side is the least (ord, bincode), the re-offer, the re-store of a peeked record, the drain, the re-issue of a divergent pair after one round, the re-issue of any chain after 64 fires, and the invariant INV at the end of every deploy. A second note gives the joins of P5 wave W3: a collection that a join waiter holds runs on the join engine, the join waiter fires only as the head waiter of each of its queue collections, m binds on one collection see its first m records, and the cascade fires a matching pair of heads that a fire exposes, in the same transaction.](diagrams/05-head-only-queue.svg)

*Figure 1. Source: [diagrams/05-head-only-queue.puml](diagrams/05-head-only-queue.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

### 3.2 Bag

A bag collection is a multiset on both sides.
The scan driver plans its operations (`rholang/src/rust/interpreter/spaces/disciplines/scan.rs:179-218`, `rholang/src/rust/interpreter/spaces/disciplines/scan.rs:229-321`), and `BagDiscipline` uses it (`rholang/src/rust/interpreter/spaces/disciplines/bag.rs:35-99`).

**Rule BG-1: canonical order.** The driver visits the records and the waiters of a collection in the canonical order of DR-75 ([decision 41](decision-register.md#decision-41)).
The order sorts by the hash of the source produce or consume of each entry, and then by digest inside a run of equal hashes. It depends on the entry set only.

**Rule BG-2: receive.** A receive takes the first record in canonical order whose payload its pattern binds.
It checks its `where` guard once, on that record, and a refusal parks the receive, whatever the later records ([decision 37](decision-register.md#decision-37)).

**Rule BG-3: send.** A send fires the first waiter in canonical order whose pattern binds the datum and whose guard admits it.
A guard refusal moves the send to the next waiter ([decision 37](decision-register.md#decision-37)).

**Rule BG-4: multiset.** A store adds one record. A fire removes the one record that it binds, unless the record is persistent. Equal sends store equal records.
For a record key $`j`$ and an operation $`o`$ on the records $`R`$:

```math
\#_j(o(R)) + \mathrm{taken}(o, R, j) = \#_j(R) + \mathrm{added}(o, j)
```

Here $`\#_j(R)`$ is the number of records with the key $`j`$ in $`R`$, and $`\mathrm{added}(o, j)`$ is 1 for a store of $`j`$.
$`\mathrm{taken}(o, R, j)`$ is 1 for a fire that binds a non-persistent record $`j`$ of $`R`$.

**Rule BG-5: peeks.** A peek fire removes a non-persistent record, as the host COMM does ([decision 45](decision-register.md#decision-45), Rule E).
The same operation sends the record again: the record is offered to the waiters first, and it is stored when no waiter takes it.
The second send has the identity of the record and registers the cost authority of the record (`rholang/src/rust/interpreter/spaces/disciplines/scan.rs:730-762`).

**Rule BG-6: peek waiters.** A send that fires a peek waiter keeps its datum in flight and offers it to the next waiter ([decision 30](decision-register.md#decision-30)).

**Rule BG-7: no counter.** A bag has no counter. Every record and every waiter has the order 0, so a re-issued element keeps its record and its identity ([decision 43](decision-register.md#decision-43)).
So two branches that write one bag merge, as on a host channel (`rholang/tests/reified_merge_p5.rs:700-755`).

**Rule BG-8: bounds.** One transaction makes at most `FIRE_ROUND_CAP` fires, the fires of a peek re-store included ([decision I-21](decision-register.md#i-21)).
A divergent pair or the cap re-issues the element in flight, and every read and copy is reserved before it is made ([decision 42](decision-register.md#decision-42)).

### 3.3 Cell

A cell collection holds at most one record and at most one waiting continuation (F15).
The family `hashmap:cell` runs `CellDiscipline` (`rholang/src/rust/interpreter/spaces/disciplines/cell.rs:53-88`) on the head driver of the engine.

**Rule CL-1: capacity.** A store or a park on a full side fails with `CellOccupied` before the plan stages it (`rholang/src/rust/interpreter/spaces/generic.rs:1118-1135`).
The job drops the transaction, so the failed operation logs no event, bumps no counter, and makes no legacy charge.

**Rule CL-2: both sides meet.** A fresh send always meets the waiting continuation of the cell, and a fresh receive always meets the record of the cell.
Only an element that does not fire is stored or parked, so a receive can fire the record while the waiter slot is full.

**Rule CL-3: counter.** Every operation that commits bumps the counter of the collection once ([decision 31](decision-register.md#decision-31)).
Two branches that operate on one cell race under merge check #1, so a merge never puts two records into one cell.
A cell operation that fails with `CellOccupied` fails its deploy, and the merge leaves a failed deploy out (`rholang/tests/reified_merge_p5.rs:759-807`).

**Rule CL-4: peeks.** A consume-side peek removes a non-persistent record, and the same transaction stores the record again with its identity (Rule E).
The fire left the record slot empty, so the re-store is never `CellOccupied`.
A produce-side peek waiter keeps the datum in flight. The datum is then stored, or the operation fails with `CellOccupied` when the record slot is full.

**Rule CL-5: re-entry.** A re-issued persistent element keeps its first-round order and identity ([decision 43](decision-register.md#decision-43)).
It re-enters as a fresh element, so an older entry on its side does not hold it back.

### 3.4 Stack

A stack collection is head-only on both sides, and its head is the entry with the greatest order.
The family `hashmap:stack` runs `StackDiscipline` (`rholang/src/rust/interpreter/spaces/disciplines/stack.rs:69-139`) on the head driver and the settle loop.

**Rule SK-1: head.** The head of each side is the entry with the greatest pair (ord, bincode bytes).
Only the head record and the head waiter interact, and a head that does not match blocks the side ([decision 11](decision-register.md#decision-11)).

**Rule SK-2: entries.** A fresh element always reaches the top of its side.
A re-issued persistent element keeps its first-round order, and it precedes the top only when that order is greater ([decision 43](decision-register.md#decision-43)).
So it re-enters at its LIFO place.

**Rule SK-3: settle.** A fire can expose an older pair of a record and a waiter below the tops.
So the stack settles before and after each plan, and in a settle job (`rholang/src/rust/interpreter/spaces/settle.rs:404-498`).
The settle fires the top pair. It lifts a stored record that is not persistent with a consume of `K_lift`, and it offers the record again with its own identity.

**Rule SK-4: peeks in a settle.** A record that a peek waiter did not spend stays in flight, and it lands at its order when the settle ends (Rule E).

**Rule SK-5: fire count.** One transaction makes at most `FIRE_ROUND_CAP` fires, its settles included ([decision I-21](decision-register.md#i-21)).
The settle before the plan, the plan and the settle after it count on one count of the transaction (`rholang/src/rust/interpreter/spaces/generic.rs:1702-1715`).
A plan whose transaction is full after the settle before it stages nothing, and its operation re-issues.

**Rule SK-6: re-issues.** At the cap or at a divergent pair, the settle before the plan re-issues the operation, which it has not inserted yet.
The settle after the plan and a settle job re-issue a settle job, because the element of the operation is spent, stored or parked.

**Rule SK-7: order.** Records and waiters leave in LIFO order, and no record and waiter pair matches when a transaction ends (`stack_lifo`, `stack_produce_preserves_inv`).

### 3.5 Set

A set collection is a bag whose records form classes of equal payloads.
The family `hashmap:set` runs `SetDiscipline` on the scan driver of the bag (`rholang/src/rust/interpreter/spaces/disciplines/set.rs:80-165`).

**Rule ST-1: key.** The class key of a payload $`p_1 \dots p_n`$ is the F5 key

```math
\mathrm{key}(p_1 \dots p_n) = \mathrm{u64}(n) \,\Vert\, \mathrm{u64}(\ell_1) \,\Vert\, b_1 \,\Vert\, \cdots \,\Vert\, \mathrm{u64}(\ell_n) \,\Vert\, b_n
```

Here $`b_i`$ is the bincode of the sorted $`p_i`$, and $`\ell_i`$ is the length of $`b_i`$ (`rholang/src/rust/interpreter/spaces/disciplines/set.rs:176-227`).
$`\mathrm{u64}`$ gives the 8 big-endian bytes of a length.
The key never includes the random state, the authority or the cost stack of a record.

**Rule ST-2: classes.** Two records are in one class exactly when their keys are equal.
The keys are equal exactly when the sorted elements of the payloads are equal (`set_key_injective`).

**Rule ST-3: store and view.** The store keeps every record of a set, also equal ones.
The view of the records has one representative for each class: its first member in the canonical order of DR-75 ([decision 41](decision-register.md#decision-41)).

**Rule ST-4: receive.** A receive takes the first record in canonical order whose payload its pattern binds, and that record is the representative of its class.
The receive checks its guard once, on that record, and a refusal parks it ([decision 37](decision-register.md#decision-37)).

**Rule ST-5: class fire.** A receive that does not peek fires one consume over $`[D; n]`$ on every stored member of the class, the representative first (F3, F20).
The fire dispatches only the result of the representative, and it removes every member that is not persistent (`rholang/src/rust/interpreter/spaces/disciplines/scan.rs:597-685`).

**Rule ST-6: send and retire.** A send fires the first waiter in canonical order whose pattern binds the datum and whose guard admits it.
After each fire of a waiter that does not peek, the send retires the non-persistent stored members of the class of its datum.
Each retire is a single-channel COMM of a `K_retire` continuation, in canonical order (`rholang/src/rust/interpreter/spaces/disciplines/scan.rs:794-879`).
A retire is never dispatched, and it is not a fire round of the transaction.

**Rule ST-7: peeks.** A peek binds only the representative, on $`[D]`$, and its fire removes a representative that is not persistent.
The same operation sends the representative again (Rule E, [decision 45](decision-register.md#decision-45)), and a peek retires nothing.
When the second send fires a waiter that does not peek, that fire retires the class (`rholang/src/rust/interpreter/spaces/disciplines/scan.rs:709-789`).

**Rule ST-8: membership.** The driver compares the metered hashes of two keys first, and it compares the full keys only when the hashes are equal (DR-90).
So a hash collision never joins two classes (`rholang/src/rust/interpreter/spaces/disciplines/scan.rs:884-899`).

**Rule ST-9: no counter.** A set has no counter, no params, no query and no capacity.
Two waiters are equal only when their bytes are equal (F14). Every order is 0, so a re-issued element keeps its identity ([decision 43](decision-register.md#decision-43)).

Without a counter, the merges of a set follow the merge checks of its records.
Writes to one set merge, and the merged view is the view of each serial order (F25).
Two branches that each retire one stored member of a set race in merge check 1 on the produce of that member (F24).

**Rule ST-10: keys under a budget.** A metered job stores only a record whose key exists under its metering (`rholang/src/rust/interpreter/spaces/disciplines/scan.rs:419-429`).
The metered sorter rejects a term deeper than its depth limit with `HostWorkRejected`, so the send of such a payload fails before it stages anything.
Without a budget the host sorter keys every payload, so a job without a budget stores every record.
A record that such a job stored can lack a key under a later budget.
Then a metered class operation that reaches the record fails before it stages anything.

The class law of a class fire (`set_consume_retires_class`) is

```math
\#_s(R \setminus C) = \begin{cases} 0 & \text{if } \mathrm{key}(s) = \mathrm{key}(k) \text{ and } s \text{ is not persistent} \\ \#_s(R) & \text{otherwise} \end{cases}
```

Here $`R`$ is the stored records, $`k`$ is the representative, and $`C`$ is the set of members that the fire removes.
$`\#_s(R)`$ is the number of records with the source $`s`$ in $`R`$.

### 3.6 Priority queue

A priority-queue collection keeps its records in levels and its waiting continuations in lanes ([decision 38](decision-register.md#decision-38)).
The family `hashmap:priorityqueue` runs `PriorityQueueDiscipline` on the head driver and the settle loop (`rholang/src/rust/interpreter/spaces/disciplines/priority_queue.rs:232-346`).

**Rule PQ-1: levels.** A send with `priority = p` puts its datum in the level $`p`$.
A send without a priority puts its datum in the unprioritized level, which comes after every priority level (F6, `rholang/src/rust/interpreter/spaces/disciplines/priority_queue.rs:119-155`).

**Rule PQ-2: lanes.** A receive with `priority = p` waits in the lane of the level $`p`$.
A receive without a priority waits in the unrestricted lane (`rholang/src/rust/interpreter/spaces/disciplines/priority_queue.rs:157-193`).

**Rule PQ-3: no storage for each level.** A record holds its level in the priority slot of its envelope, and a continuation holds its lane in its policy slot (`rholang/src/rust/interpreter/spaces/record.rs:867-1233`).
So the priority queue keeps no storage for each level (I-10).

**Rule PQ-4: heads.** Each level and each lane is a FIFO queue whose head has the least pair (ord, bincode bytes) (`rholang/src/rust/interpreter/spaces/disciplines/priority_queue.rs:355-397`).
The lane of the level $`p`$ sees the head of the level $`p`$ (`rholang/src/rust/interpreter/spaces/disciplines/priority_queue.rs:186-192`).
The unrestricted lane sees the heads of all levels in ascending level order (`rholang/src/rust/interpreter/spaces/disciplines/priority_queue.rs:513-539`).

**Rule PQ-5: send.** A send offers its datum to the head of the lane of its level and to the head of the unrestricted lane.
The older head that fits fires, and a refusal moves the send on to the other head (`rholang/src/rust/interpreter/spaces/disciplines/priority_queue.rs:545-597`, `795-854`).

**Rule PQ-6: receive.** A receive binds the first matching level head that its lane sees, and it checks its guard once, on that head.
A refusal parks the receive (`rholang/src/rust/interpreter/spaces/disciplines/priority_queue.rs:860-965`).

**Rule PQ-7: peeks.** A peek of a record that is not persistent stores the record again at once, at its order and in its level (Rule E, `rholang/src/rust/interpreter/spaces/disciplines/priority_queue.rs:970-979`).

**Rule PQ-8: settle.** A fire can expose a pair, so the priority queue settles before and after each plan and in a settle job (`rholang/src/rust/interpreter/spaces/disciplines/priority_queue.rs:319-345`).
Strict confluence fails for a priority queue (readiness review S4), so the settle fires the oldest fireable lane head first (`rholang/src/rust/interpreter/spaces/disciplines/priority_queue.rs:1010-1105`).
When that fire binds a stored record while a record is in flight, the record in flight lands first (`rholang/src/rust/interpreter/spaces/settle.rs:425-430`).

**Rule PQ-9: params.** The send param and the query param `priority` take a `GInt` of at least 0 (`rholang/src/rust/interpreter/spaces/disciplines/priority_queue.rs:750-762`).
When the space sets the bound `priorities`, the value must be below it, and otherwise the operation fails with `InvalidParam`.
A positional query part fails with `PositionalParamsUnsupported`.

**Rule PQ-10: fire count and re-issues.** The rules SK-5 and SK-6 of the stack hold for the priority queue.
A re-issued element takes its FIFO place in its level, and a re-entered waiter keeps its seniority in its lane ([decision 43](decision-register.md#decision-43)).

### 3.7 Receive queries

**Rule RQ-1: inner channel.** A receive bind with a QUERY tuple as its source reads from the inner channel of the query (`rholang/src/rust/interpreter/reified/reducer_hooks.rs:1166-1180`).
The inner channel takes the read rule of a receive source.

**Rule RQ-2: route.** The route of the inner channel decides (`rholang/src/rust/interpreter/reified/reducer_hooks.rs:1198-1243`).
A native space gets the payload and the named params in its consume, and its discipline checks them before the bump.
The default space fails with `QuerySpecUnsupported`, and a user agent fails with `QuerySpecUnsupportedOnAgentSpace` ([decision 77](decision-register.md#decision-77), item TR-2).

**Rule RQ-3: no host channel.** A QUERY tuple never becomes a channel of the host (`rholang/src/rust/interpreter/reified/reducer_hooks.rs:1153-1155`).

### 3.8 Joins

A [join](glossary.md#join) receives on two or more channels at once.
P5 wave W3 lets a join read the collections of one native space, for every discipline ([decision 29](decision-register.md#decision-29)).
The [join engine](glossary.md#join-engine) runs every operation that a join can reach, and the drivers of sections 3.1 to 3.6 run every other operation.
The rules below use the group $`G`$ of a waiter $`W`$: the data channels of its binds, in canonical order.
For a collection $`c`$, $`m_W(c)`$ is the number of positions of $`W`$ on $`c`$.

**Rule JN-1: route.** A receive with two or more binds whose channels all route to one native space is a join of that space.
The join keeps its binds in the order of the receive, each with its query (rule RQ-1).
A join must abort with `CrossSpaceJoin` when its channels name two spaces, a space and the default space, or an agent and another target.
A join whose channels all route to one user agent is the translation of section [6](#6-constructs-that-fail-closed).

**Rule JN-2: checks and scope.** A join checks the key of each bind with the outer store and the query of each bind with the discipline, before it stages anything.
The [join scope](glossary.md#join-scope) of an operation is the closure of its collections under the join groups of their data channels.
The transaction of the operation locks every collection of its scope before it stages anything.
A join group with a channel outside the space makes the state `Inconsistent`, and the operation must abort.

**Rule JN-3: group and record.** The waiter of a join waits on its group, which is a [canonical group](glossary.md#canonical-group) ([decision 28](decision-register.md#decision-28)).
A channel can occur more than once in a group, and $`k`$ binds on one collection bind $`k`$ distinct records.
The record of the waiter holds one [join slot](glossary.md#join-slot) for each position of its group.
A fire puts its results back into the bind order of the receive by the slots.

**Rule JN-4: one arrow.** One arrow covers every bind of a join, so the binds all consume, all persist, or all peek.

**Rule JN-5: counters and orders.** A join bumps the counter of each distinct collection of its binds once, when the discipline has a counter (rule OC-1).
The join takes the value of each bump as its order in that collection, and a bag or a set gives the order 0.
A re-issued persistent join keeps the order of its first round in each collection ([decision 43](decision-register.md#decision-43)).
Every other collection that the transaction stages on is bumped once, before its first staging.

**Rule JN-6: windows.** The positions of $`W`$ on $`c`$ see a window of the entries of $`c`$.
On a queue, a stack, or a cell, the window is the first $`m_W(c)`$ entries of the head order of $`c`$.
On a priority queue, the window is the first $`m_W(c)`$ records of each level that the lane of the position sees.
On a bag or a set, the window is every record of $`c`$, in the canonical order of DR-75 ([decision 41](decision-register.md#decision-41)).
An element in flight takes its place in the window of its collection.

**Rule JN-7: heads.** A join waiter fires only when it is the head waiter of each head-only collection of its group ([decision 11](decision-register.md#decision-11)).
On a priority queue, the waiter must be the head of its lane.
A fresh waiter parks at the tail of each of its collections, and a re-issued waiter takes the places of its carried orders.

**Rule JN-8: consume-side search.** The search takes the positions of $`G`$ in order, and it never goes back to an earlier position ([decision 37](decision-register.md#decision-37)).
Each position binds the first unclaimed entry of its window that its pattern binds, and a set position claims the whole class of the entry.
The guard of the join runs once, over the bound values in bind order.
A position without an entry, or a refusal of the guard, parks the waiter.

**Rule JN-9: produce-side search.** A send offers its datum to the waiters of its collection, in the order of the discipline.
A queue, a stack, or a cell offers it to the head waiter.
A priority queue offers it to the heads of two lanes, the older head first.
A bag or a set offers it to every waiter, in the canonical order of the union of its groups.

On a head-only collection a refusal of the head stores the datum ([decision 11](decision-register.md#decision-11)).
Otherwise a refusal moves on to the next waiter ([decision 38](decision-register.md#decision-38)).
For each waiter, the datum binds exactly one position ([decision 28](decision-register.md#decision-28)).
The search tries each position where the datum enters the window, in group order, and it fills the other positions as rule JN-8 does.
So the datum can bind a later position when an earlier position of its collection gives no complete match.

**Rule JN-10: cascade.** A fire can make a parked waiter of the scope fireable ([decision 50](decision-register.md#decision-50)).
The fire can give a head-only collection a new head, or it can move a set class to a new place.
The join engine fires each such waiter in the same transaction, and it visits the changed collections in ascending key order.
On each collection, it fires the head waiter, the oldest lane head of a priority queue, or the first set waiter in canonical order.
It lifts the first non-persistent record that the waiter binds, and it offers that record to the waiter again with its identity and its authority.

A persistent record is never lifted, and its re-offer leaves it stored.
A bag has no cascade, because a removal never gives a bag waiter without a guard a new match.
The engine also runs the cascade before the plan of each operation on a space whose discipline cascades.

**Rule JN-11: set classes.** After a fire of a waiter that does not peek, each bound set class retires its other non-persistent stored members (rule ST-6).
Each retire is a single-channel COMM of a `K_retire` continuation.
A single-bind receive on a joined set keeps its class fire over $`[D; n]`$ (rule ST-5).

**Rule JN-12: peeks.** A join that peeks removes each bound non-persistent stored record in its COMM, as the host COMM does.
The same transaction sends each such record again through rule JN-9, in receipt order ([decision 45](decision-register.md#decision-45), Rule E).
After a peek fire, the element in flight is offered again, also when it is persistent ([decision 30](decision-register.md#decision-30)).

**Rule JN-13: fire count and re-issue.** A transaction of the join engine makes at most `FIRE_ROUND_CAP` fires ([decision 49](decision-register.md#decision-49)).
At the cap it commits its fires and re-issues one settle job.
The job carries the owed second sends, the collections that can still owe a fire, and the re-issue of the operation, in this order.
A divergent pair does not stop the transaction, and the operation re-issues itself when the cascade ends (P4 item A10).
A stop that owes nothing re-issues only the operation, or nothing when the operation is complete.

**Rule JN-14: invariant.** Let a transaction of the join engine end with nothing owed.
Then no waiter of its scope can fire on a queue, stack, cell, priority-queue, or set space, also when the waiter has a guard.
On a bag space, no waiter of the scope without a guard has a match, and a guarded waiter can keep a match ([decision 37](decision-register.md#decision-37)).
Each re-issue runs in the same deploy, so a completed deploy owes nothing.

**Rule JN-15: merges.** A join transaction bumps the counter of each collection that it stages on, so two branches that touch one ordered collection race (rule OC-5).
A parked join waiter makes every later produce on its channels conflict with every other branch under merge check 3, as a host join does.
Bag and set joins race under merge checks 2 and 3, as host joins do.
Only a parked join of a program leaves a join entry of two or more channels ([decision I-16](decision-register.md#i-16)).

**Example 1: a cascade on two queue collections.** Table 7a shows how one send fires a join and how the cascade then fires a waiter that the join fire exposed.
The waiter W1 is the join `for (@x <- a & @y <- b)`, and the waiter W3 is `for (@u <- b)`.

*Table 7a. A cascade on two queue collections.*

| Step | Event | State after the step |
|---|---|---|
| 0 | The start. | `a` is empty, and `b` holds 5, then 6. The waiters of `b` are W1, then W3, so W1 blocks W3. |
| 1 | The send `a!(1)` offers 1 to W1, the head waiter of `a` and of `b`. The search of W1 binds 1 and the head 5 of `b`. | W1 fires. `b` holds 6, and its head waiter is W3, which matches 6. |
| 2 | The cascade lifts 6 with `K_lift` and offers 6 to W3 again. | W3 fires with `u = 6`, and both collections are empty. |

Without the cascade, the heads of `b` would keep a waiter and a record that match, and rule JN-14 would fail.

**Example 2: a class that moves in a set.** Table 7b shows the case of [decision 50](decision-register.md#decision-50).
The waiter W is the join `for (@x <- s & @1 <- s)` on one set collection `s`, and V is the receive `for (@1 <- s)`.

*Table 7b. A cascade on a set collection.*

| Step | Event | State after the step |
|---|---|---|
| 0 | The start. | `s` holds `a1` with 1, `b` with 2, and the persistent `a2` with 1, in this canonical order. The class of 1 sits at the place of `a1`. |
| 1 | W arrives. Its first position takes the class of 1, and its second position finds no other class of 1. | W parks. |
| 2 | V fires with the class of 1. | `a1` leaves and `a2` stays, so the class of 1 sits at the place of `a2`, after `b`. |
| 3 | The cascade finds that W can fire now: its first position takes `b`, and its second position takes the class of 1. | The engine lifts `b` and offers it to W, so W fires with `x = 2`. `s` holds `a2`. |

### 3.9 VectorDB

A VectorDB collection holds records that can carry a vector, and a receive selects records by a similarity query.
The FIPS puts the data into a vector DB, and a `for` consumes one vector within a distance bound of the query (`Reifying RSpaces.md:255-257`).
The families `hashmap:vectordb` and `vector:vectordb` run `VectorDbDiscipline` (`rholang/src/rust/interpreter/spaces/disciplines/vectordb.rs:156-276`).
A collection that a join reaches runs on the join engine instead ([section 3.8](#38-joins)).
Section [N15 of the design](design.md#n15-phase-p6-the-vectordb-family) explains the code, and decisions 54, 56, 60 and 66 to 71 of the [register](decision-register.md) fix the behavior.

*Table 8. The items of the extension point for VectorDB.*

| Item | VectorDB |
|---|---|
| The head rule of each side | No head. A receive selects by its query (rules VD-4 to VD-6), and a send offers itself to the parked waiters in the canonical order (rule VD-7). |
| The counters of rules OC-1 to OC-6 | None. Every record and every waiter has the order 0 (rule VD-12). |
| Joins | A bind in single mode joins other channels of the space. A bind in list mode fails with `ListModeInJoin` (rule VD-11). |
| Named send params | `embedding` (rule VD-2) |
| The query | A query vector and the keys `metric`, `threshold`, `radius`, `k`, `results`, `scores` and `ids` (rule VD-3) |
| The rules QD-1 to QD-9 | QD-4 and QD-5 hold. Rules VD-4 to VD-7 replace QD-1 to QD-3 and QD-7, rules VD-2, VD-3 and VD-11 replace QD-8, rule VD-10 replaces QD-6, and QD-9 holds trivially with the order 0. |

**Rule VD-1: factory params.** The factory takes `capacity` (required, an `Int` in $`[1, 1\,000\,000]`$) and `dimensions` (required, an `Int` in $`[1, 4\,096]`$).
It also takes `metric` (default `cosine`), one of `cosine`, `dot`, `euclidean`, `manhattan`, `hamming` and `jaccard`, and `mirror`, which must be `"chroma"` (`rholang/src/rust/interpreter/spaces/factory/defaults.rs:63-102`).
A space with `mirror` must have the default qualifier and a metric that ChromaDB indexes: `cosine`, `dot` or `euclidean`. Otherwise the factory fails with `InvalidParam` on `mirror` (gate 9, `rholang/src/rust/interpreter/spaces/factory/constructor.rs:229-260`).

**Rule VD-2: records.** A send stores its payload with an optional vector (`rholang/src/rust/interpreter/spaces/disciplines/vectordb.rs:635-657`).
The vector is the param `embedding`, or else the payload itself when the payload is exactly one non-empty list of numbers ([decision 66](decision-register.md#decision-66)).
A vector must have the dimensions of the space (`DimensionMismatch`), and its elements must be finite (`NonFiniteValue`).
Any other payload has no vector, so no query with a vector selects it.
The record keeps the vector in its envelope, and Rule E keeps the vector of a record that a peek stores again.

**Rule VD-3: queries.** A receive bind with the query `~ (v; named)` reads the collection of its inner channel ([section 3.7](#37-receive-queries)).
The checks run before any staging, in this order, so a query with several faults reports one code (`rholang/src/rust/interpreter/spaces/disciplines/vectordb.rs:404-511`):

1. Without a query vector, the keys `ids`, `metric`, `radius`, `scores` and `threshold` fail with `QueryNeedsVector`. The keys `k` and `results` stay valid.
2. `metric` must name one of the six metrics (`InvalidParam`). Without it, the metric of the space applies.
3. The query vector must have the dimensions of the space, and its elements must be finite. A zero vector fails with `ZeroQueryVector` for `cosine`.
4. `threshold` and `radius` exclude each other (`ConflictingParams`). `threshold` is a number in $`[0, 1]`$ for a similarity metric (`ThresholdNeedsSimilarity`), and `radius` is a number $`\ge 0`$ for a distance metric (`RadiusNeedsDistance`).
5. `k` is an `Int` in $`[1, \mathit{capacity}]`$, `results` is the text `"all"`, and the two exclude each other.
6. `scores` is a `Bool`, and `ids` is a list of 32-byte byte arrays.

The similarity metrics are `cosine`, `dot` and `jaccard`, and the distance metrics are `euclidean`, `manhattan` and `hamming`.
Only a linear bind with a simple source takes a query in source text ([syntax contract](syntax-contract.md)).
So only the agent API can give a query to a peek or a persistent receive.
A source program reads the canonical order with them.

**Rule VD-4: the eligible records and their order.** For a query $`Q`$, the eligible set $`E`$ holds each record that passes three filters (`rholang/src/rust/interpreter/spaces/disciplines/vectordb.rs:881-952`):

- With `ids`, the hash of the source produce of the record is one of the ids.
- With a query vector, the score of the record is defined. So the record has a vector, a `cosine` record has a norm above 0, and a `hamming` or `jaccard` record is binary.
- The score passes the bound `threshold` or `radius`.

The records start in the canonical order of DR-75 ([decision 41](decision-register.md#decision-41)).
With a query vector, a stable sort puts the better exact score first, and equal scores keep the canonical order (`rholang/src/rust/interpreter/tensor/exact.rs:478-521`).
A higher similarity and a lower distance are better.
Without a query vector, $`E`$ keeps the canonical order, so the receive is a list read of the records.
Figure N15.2 of the design shows the selection.

**Rule VD-5: single mode.** A receive without `k` and `results` takes the first record of $`E`$ whose payload its pattern binds (`rholang/src/rust/interpreter/spaces/disciplines/vectordb.rs:1655-1707`).
It checks its `where` guard once, on that record, and a refusal parks the receive ([decision 37](decision-register.md#decision-37)).
The fire is one COMM over $`[D]`$. With `scores = true`, the receive binds `[score, value]`.
Without a record, the receive parks, with its query in the policy slot of its envelope.

**Rule VD-6: list mode.** A receive with `k` or `results = "all"` binds one value: the list of the first $`\min(k, |E|)`$ records of $`E`$, or of all of $`E`$ (`rholang/src/rust/interpreter/spaces/disciplines/vectordb.rs:1068-1117`).
Each element of the list is the payload value of a record, or `[score, value]` with `scores = true`.
The payload value of a send with one datum is that datum.
The payload value of a send with $`n`$ data is the list of the $`n`$ data.

The receive fires when the list is not empty, its pattern binds the list, and its guard admits the list. The guard is checked once.
The fire is one COMM over $`[D; m]`$ for the $`m`$ records of the list. Otherwise the receive parks on $`[D]`$ with its query (LM-b, [decision 66](decision-register.md#decision-66)).

A list of one member keeps the random state of that member.
A longer list merges the random states of its members in delivery order (`rholang/src/rust/interpreter/spaces/disciplines/vectordb.rs:810-850`).

**Rule VD-7: sends.** A send offers its datum $`x`$ to the parked waiters in the canonical order (`rholang/src/rust/interpreter/spaces/disciplines/vectordb.rs:1422-1471`):

- A single-mode waiter takes $`x`$ when $`x`$ is eligible for its query, its pattern binds $`x`$, and its guard admits.
- A list waiter $`W`$ takes $`x`$ when $`x`$ is in the list $`L(W, R + x)`$, its pattern binds that list, and its guard admits.
- A waiter without a query takes $`x`$ as a bag waiter does.
- A guard refusal moves the send to the next waiter ([decision 37](decision-register.md#decision-37)).

The fire is one COMM over $`[D]`$ that binds $`x`$.
When a list waiter does not peek, each other non-persistent member of its list leaves through a `K_retire` COMM in the same transaction (`rholang/src/rust/interpreter/spaces/disciplines/vectordb.rs:1151-1194`).
A send that no waiter takes is stored, below the capacity (rule VD-12).

**Rule VD-8: fires and rounds.** Table 6 of rule QD-4 decides the survivors of each fire, and rule QD-5 decides the next step.
One fire count runs through the transaction, its tail included ([decision 49](decision-register.md#decision-49)).
A divergent pair re-issues the operation at the end of the transaction, after the tail. A stop at `FIRE_ROUND_CAP` carries the owed work in one settle job.
A re-issued element keeps its record and its identity ([decision 43](decision-register.md#decision-43)).

**Rule VD-9: peeks.** A peek fire removes each non-persistent record that it binds, as the host COMM does ([decision 45](decision-register.md#decision-45), Rule E).
The same transaction sends each removed record again with its vector, the last owed record first (`rholang/src/rust/interpreter/spaces/disciplines/vectordb.rs:1791-1826`).
A send that fires a peek waiter keeps its datum in flight and offers it to the next waiter ([decision 30](decision-register.md#decision-30)).

**Rule VD-10: LIST-CASCADE.** After each removal, the tail checks the parked list waiters in the canonical order (`rholang/src/rust/interpreter/spaces/disciplines/vectordb.rs:1878-1978`).
It fires the first waiter whose list $`L(W, R)`$ is not empty, binds its pattern and passes its guard.
The fire lifts the first member of the list with `K_lift`, when the member is not persistent, and offers it to the waiter.
The tail repeats until no parked list waiter can fire, or until the fire count reaches `FIRE_ROUND_CAP`.

So at the end of each transaction, no parked waiter without a guard can fire on the records (`vdb_no_pending_match`, S22 of `VectorDbDiscipline.v`).
A waiter that its guard refused on a better record can still fire later on a worse one ([decision 37](decision-register.md#decision-37)).

**Rule VD-11: joins.** A join of the space can name a VectorDB collection in single mode ([section 3.8](#38-joins)).
Each bind sees its own eligible records in its own order, and the engine fires the join over one record of each bind (`JoinWindow::Similarity`).
A removal never makes a single-mode join waiter without a guard fireable, so single-mode joins need no cascade.
A bind in list mode fails the join with `ListModeInJoin`, because a list is one value of one bind (`rholang/src/rust/interpreter/spaces/join.rs:412-471`).
Without a query, a VectorDB bind of a join reads the canonical order.

**Rule VD-12: capacity and merges.** A VectorDB collection has no counter, so every record and every waiter has the order 0.
A store fails with `CapacityExceeded` when the collection holds `capacity` records, before anything is staged (`rholang/src/rust/interpreter/spaces/generic.rs:1118-1135`).
A store of Rule E never exceeds the capacity, because its COMM removed the record in the same transaction.

Two concurrent branches that write one collection merge with no conflict, as on a host channel.
So `capacity` is a soft bound ([decision 71](decision-register.md#decision-71)).
A merge can exceed it by the stores of the merged branches.
Every store then fails until removals bring the count below `capacity`.

**Rule VD-13: delivered values.** Every decision of the discipline compares exact values ([decision 54](decision-register.md#decision-54)).
A delivered score is the `f64` nearest to the exact value, with ties to even, written $`\mathrm{RN}_{53}`$ (`rholang/src/rust/interpreter/tensor/exact.rs:578-599`).
`cosine` and `euclidean` deliver the correctly rounded square root of an exact value.
A delivered value beyond the finite range of `f64` fails with `ValueOutOfRange`.
Figure N15.1 of the design shows the number model.

**Rule VD-14: work in native execution.** Every read, decode, comparison and copy of the discipline is reserved before it runs ([decision 42](decision-register.md#decision-42)).
At the v6 limits of native execution, a natively certified deploy holds only a small VectorDB collection.
In native play, 8 records and 8 list waiters use 0.54 of the limit of `VerificationBytes`, and 16 of each exceed it ([verification section 16](verification.md#16-phase-p6-step-5-native-certification-of-the-vectordb-shapes)).
A program at the round cap needs about 19 times that limit in native replay.

## 4. Qualifiers

A [qualifier](glossary.md#qualifier) sets the persistence and the concurrency of a space.

### 4.1 Default

**Rule QF-1: the default qualifier.** A constructor without a qualifier creates a space with the qualifier `"default"`.
The constructor `"default"` has the same effect.

**Rule QF-2: durable.** A default space is durable.
Its ids use the uppercase [KIND](glossary.md#kind) bytes, and its records are ordinary channels of the RSpace of the node.
A checkpoint and a restart must keep its config record, its counters, its records, and its waiters.
After a restart, the first operation reads the space from the trie and pays what a node without a restart pays (NE-2).

**Rule QF-3: concurrent.** A default space accepts operations from parallel branches of one deploy and from many deploys.
The scheduler orders the operations of one space, and it runs the operations of different spaces in parallel.

**Rule QF-4: families.** Every outer store takes the qualifiers `"default"`, `"temp"` and `"seq"`, except the HashSet store, which takes only `"seq"`.
A HashSet constructor with another qualifier must abort with `BadConstructorArgument`, in native and in legacy execution.

### 4.2 Temp

A temp space lives for one deploy (FIPS 275-277, [decision I-4](decision-register.md#i-4)).
Phase P7 adds it ([decision 73](decision-register.md#decision-73)).

**Rule QF-5: one deploy.** A constructor with the qualifier `"temp"` creates a space that lives until the end of the deploy that created it.
Its state stays in the hot store of the node and never reaches a checkpoint.

**Rule QF-6: ephemeral ids.** Every id of a temp space uses the lowercase form of its [KIND](glossary.md#kind) byte, `kind | 0x20`.
Its space id comes from the message that created it (SM-1), so two deploys never share an id.

**Rule QF-7: events and charges.** An operation on a temp space emits the events and the observer calls of the same operation on a default space.
Replay pairs these events, and the deploy pays for them.
A send pays its introduction also when the sweep later removes its record, so no refund applies.

**Rule QF-8: the sweep.** Each root evaluation keeps a book of the temp spaces that it created and of the volatile channels that its space transactions locked.
After every detached task of a root evaluation, the node [sweeps](glossary.md#sweep) the channels of the book: it removes their records, waiting continuations and join entries.
The sweep runs after a success and after a failure, under the boundary guard of the reducer, and it emits no event and no charge.
Play, replay and native replay sweep at the same point, so their post-states are equal.
A failed sweep breaks an invariant of P7, and the deploy fails with `BugFoundError` of the class `Platform`.

**Rule QF-9: dangling ids.** An id of a temp space that the current root evaluation did not create belongs to a deploy that has ended.
A send or a receive on a name of such a space aborts with `DanglingName`.
A call on its handle, a `use` block and a `new x : s` binding abort with `DanglingHandle`.
`getSpaceAgent` replies the typed error `DanglingHandle`.
Each error names the space id in lowercase hex, and the check costs nothing beyond the evaluation that reaches it.

**Rule QF-10: no mirror.** A temp space takes no `mirror` param, so its constructor aborts with `InvalidParam` on `mirror` (gate 9).

### 4.3 Seq

A seq space is a temp space whose channels are linear (FIPS 279-285, [decision 73](decision-register.md#decision-73)).
A name of a seq space is never sent, and each channel of the space is used by one sequential process.

**Rule QF-11: native execution.** A constructor with the qualifier `"seq"` creates a seq space in native execution.
Legacy execution must refuse it with `QualifierUnavailable`, because host operations bypass the scheduler there.
The rule also holds for every HashSet family, which takes only `"seq"`.

**Rule QF-12: one deploy.** A seq space is a temp space, so the rules QF-5 to QF-10 apply to it.

**Rule QF-13: seq names.** A seq name is a name `n(sid…)` of a seq space that the current root evaluation created.
The handle of a seq space is not a name, so a program may send it.

**Rule QF-14: never sent.** While the root evaluation holds a seq space, the node checks the data of every send before any mutation.
A send whose data holds a seq name at any depth must abort with `SeqSent`, and the error names the space id.
The check covers data, quoted processes, bundles, collections and the message of a handle call.

Two sends are exempt from the check of QF-14.
In a send to the handle of a seq space, the channel argument of the method `produce` may hold names of that space.
The forwarder of a handle continuation sends the arguments of a call to the handle process, and the node does not check it again.
The replies of system processes are not sends of the program, so the reply of `gensym` carries its seq name.

**Rule QF-15: one chain of uses for each channel.** Each channel of a seq space has its own claim: the participant path of its last use.
An operation may use a channel only when the channel has no claim or the operation descends from its last use.
Otherwise the deploy must abort with `SeqConcurrent`, and the error names the space id.

A program operation uses each channel where it stores a record, parks a waiter, or fires.
A send uses the data channel of its datum.
A receive uses the channel where it waits, and a PathMap receive waits on its prefix channel.
A fire uses the channels of its waiter and the channels of its records.

**Rule QF-16: no use.** The methods `gensym`, `config` and `qualifier`, the allocations of `new x : s`, the factory, `getSpaceAgent` and the entry of a `use` block use no channel.
A re-issue at a divergent pair or at the round cap and a settle job are internal jobs, so they use no channel either.

**Rule QF-17: claims after success.** An operation stores its claims only after its commit succeeded.
An operation that fails, or a transaction that the node drops, changes no claim.

**Rule QF-18: consequences.** A routed send `x!(v)` ends the chain of `x`, because a send has no continuation and a split removes its parent.
A parked receive on `x` also ends the chain of `x`, and it never fires.
A program uses a channel again with sync handle calls such as `S!?("produce", *x, m); P`, because `P` runs below the reply of the call.

**Rule QF-19: charges.** The check of a send reserves one `VerificationOperations` unit for each node of its data.
It also reserves the `VerificationBytes` of each id and the `SearchStateBytes` of its work list.
The check of a claim reserves its lookups and its comparison of participant paths on the meter of its job.

## 5. Theories

A factory call may name a builtin theory with `free Th()`, where `Th` is one of `Nat`, `Int`, `String`, `Bool`, and `Any` ([decision 9](decision-register.md#decision-9), item 1).
The theory decides which values a send may add to the space and which stored data a receive may take.
The [theory seam](theory-seam.md) explains the design and the data flow, and [F1r3lang integration](f1r3lang-integration.md) states the plan for installed theories.

**Rule TH-1: builtins.** A builtin decides a value by its shape alone.
The predicate $`\mathrm{sole}(v)`$ holds when $`v`$ has exactly one expression, every other field of $`v`$ is empty, and $`v`$ uses no connective.
Table 11 gives the values of each builtin.
So the string `"Nat(42)"` is not in `Nat`, and neither is the par `1 | 2` or `Nil`.

*Table 11. The builtin theories.*

| Theory | A value $`v`$ is in the theory when |
|---|---|
| `Nat` | $`\mathrm{sole}(v)`$, and the expression is a `GInt` of at least 0 or a non-negative `GBigInt` |
| `Int` | $`\mathrm{sole}(v)`$, and the expression is a `GInt` or a `GBigInt` |
| `String` | $`\mathrm{sole}(v)`$, and the expression is a `GString` |
| `Bool` | $`\mathrm{sole}(v)`$, and the expression is a `GBool` |
| `Any` | Always |

**Rule TH-2: identity.** The config of a space holds the [identity](glossary.md#theory-identity) of its theory, never a handle, a token, or an epoch.
The identity has eight fields.
They are a version, a language fingerprint, a category, a category name, a theorem id, a checker ABI, a limit profile, and the arguments.
A builtin has the language `"rrs:builtin"`, the category `"Proc"`, and the ABI `"rrs:structural:v1"`.
Its limit profile is one work unit and no evidence, and it has no arguments.

**Rule TH-2a: identity bytes.** The config slot `"theory"` holds `Nil` or one `GByteArray` with the canonical bytes of the identity ([decision 74](decision-register.md#decision-74)).
The bytes hold the eight fields in their order of declaration.
The version is an `i64` and each limit is a `u64`, all in big-endian form.
Each string and each argument comes after its length, which is a big-endian `u32`, and the arguments come after their count.
The decoder accepts exactly the image of the encoder, so a decoded identity encodes to its input bytes.

The `config` reply of a handle shows the identity as a map with the keys `version`, `languageFingerprint`, `category`, `categoryName`, `theoremId`, `checkerAbi`, `limitProfile`, and `args`.

**Rule TH-3: resolution.** Each config read resolves the identity to a checker of this build.
This build resolves only the structural builtins: the builtin fields with a builtin theorem id, no arguments, and any limit profile.
Every other identity fails with `TheoryUnavailable`, which names its theorem id.
The resolution is a function of the identity alone.
So a restarted node, a validator in replay, and a node that attaches later resolve one identity to one checker.

**Rule TH-4: the factory gate.** Gate 4 of a constructor (Rule FA-6) accepts a builtin name without arguments, and the config stores its identity.
A builtin with arguments fails with `BadConstructorArgument`.
Any other name fails with `UnknownTheory`, which names the nearest builtin within two edits, or no builtin.

**Rule TH-5: admission of a send.** On a space with a theory, the checker admits each value of a send in order.
The admission runs after the config read and the gates of the send, and before any staging or counter bump.
The first value that the checker refutes aborts the deploy with `TheoryRefuted`.
The first value that the checker cannot decide aborts the deploy with `TheoryUndetermined`.
Both errors name the theory and the index of the value.

The admission covers a routed send, the handle `produce`, and each round of a re-issued send.
A second send of Rule E, a re-store, and a re-offer carry admitted data, so they are not admitted again.

**Rule TH-6: check at a match.** After the `where` guard admits a match, the checker checks each stored datum of the match with the values that its bind captures.
A verdict other than `Proven` is a guard refusal: a scan receive parks, a head blocks, and a join keeps the search of [decision 37](decision-register.md#decision-37).
A datum in flight is not checked, because its send admitted it.
Such a datum is the datum of a send, or a record that a cascade or a re-offer carries.

In list mode, each stored member is checked, and one refusal refuses the list.
On a PathMap space, the checker checks the own payload of the datum, not its re-rooted value.
A builtin checks each stored value again, so a datum that it admitted is always `Proven`.

**Rule TH-7: attach.** Each config read decodes the identity and resolves it (Rule TH-3), at the charge of the read.

**Rule TH-8: charges.** A theory check adds no phlo, because the introduction bytes of a send already pay for its values.
Each check of a builtin reserves one `VerificationOperations` unit before it reads the value.
The reservation is the same on a hit and a miss, and in play and replay.
A limit profile of zero units gives `Undetermined` before any reservation.
The identity bytes make a config read less than a tenth more expensive ([decision 74](decision-register.md#decision-74)).

**Rule TH-9: leases.** A checker may attach a lease to a `Proven` verdict, which the commit must revalidate under the authority of its language.
A transaction with two or more leases aborts with `NestedCommitGuard` before its commit, until the guard composition of P12.
The builtins attach no lease.

**Rule TH-10: test checkers.** The checkers with a lease, with a refusal at a match, or with a starved limit profile exist only in test builds.
So the library tests of `spaces/theory` check them.

## 6. Constructs that fail closed

P4 rejects each construct that a later phase adds with a deterministic code ([decision 34](decision-register.md#decision-34), item A9).
The [syntax contract](syntax-contract.md#meaning-at-run-time) lists each construct with its code and its phase.
Phase P7 replaced the code `TheoryUnavailable` of a theory in a constructor with the rules of section [5](#5-theories).
The code stays for an identity that names no checker of this build (Rule TH-3).
Phase P8 replaces the code `AgentSpacesUnavailable` with the rules of spaces that user agents implement.

Since P8 step 2, a send on a space of a user agent becomes a method call on the agent.
Items TS-1 to TS-3 and R7A-1 of [decision 77](decision-register.md#decision-77) define the call.

Since P8 step 3, a receive on a space of a user agent becomes a translation, which items TR-1 to TR-8 of decision 77 define.
These rules hold:

1. **One agent.** The channels of every bind must route to one agent, whose references are equal sorted `Par`s. A mix of targets must abort with `CrossSpaceJoin`, and a query must abort with `QuerySpecUnsupportedOnAgentSpace`.
2. **One guard owner.** The guard goes to the one bind that owns the binders that it reads, or to bind 0 when it reads none. A guard that reads the binders of two or more binds must abort with `AgentGuardSpansJoin`.
3. **Self-describing matchers.** The id of the matcher of bind $`i`$ holds the canonical state of the bind. A state that prost cannot decode must abort with `AgentPatternTooDeep` before any receive.
4. **The call comes last.** The matcher receives and the body receive come first, and the call `A!(sink, "consume", pairs, arrow, b)` comes last, because only the call can fire.
5. **Body equivalence.** The body capability binds one map from the global position of each binder to its value. So the body binds the values that the native receive binds.

A native handle runs the receive that the matchers of one translation describe (items HC-1 to HC-3).
It replies `MatcherSetMismatch` when the matchers do not describe one receive.
The [agent API](agent-api.md#5-handle-methods) gives the checks of the handle method `consume`.

## 7. Conformance

Table 9 maps each rule to the formal evidence and to the tests that check it.
The [verification dossier](verification.md#11-phase-p4-the-hashmapqueue-slice) gives the commands and the results.
A Rocq theorem shows the file and the line of its statement, and a TLA+ invariant shows its id in the invariant map.

*Table 9. The rules of this specification and their evidence.*

| Rule | Formal evidence | Tests |
|---|---|---|
| SM-1 | `sid_derivation_deterministic` (`SpaceFactory.v:669`), `space_already_exists_is_deterministic_abort` (`SpaceFactory.v:816`) | `prop_sid_derivation_deterministic`, `space_already_exists_aborts` |
| SM-2 | `counter_and_config_roundtrip` (`RecordEnvelope.v:1075`), the TLA+ invariant `HandleUnique` (U5) | `prop_counter_and_config_records_roundtrip`, `space_meta_channels_config_record` |
| SM-3 | The TLA+ invariant `DanglingIsDeterministicError` (U6) | `reified_handle_dangling_aborts`, `space_queue_dangling_name_aborts`, `reducer_dangling_handle`, `reducer_attach_costs` |
| SM-4, SM-5 | `storage_id_injective` (`Namespace.v:732`), `space_and_collection_counters_distinct` (`Namespace.v:789`) | `space_meta_channels_counters`, `space_queue_events_namespaced` |
| SM-8 | The P1 headlines of the substrate, which section [4 of the dossier](verification.md#4-rocq-models) lists | `space_queue_observer_parity`, `reified_p4_smoke_hashmap_queue_play_and_replay` |
| SM-9 | `queue_plan_permutation_invariant` (`HeadOnlyDisciplines.v:1552`) | `prop_queue_plan_permutation_invariant`, `space_queue_ord_survives_restart` |
| SM-10 | `route_rules` (`SpaceRefs.v:760`), `space_name_route_ignores_ctx` (`SpaceRefs.v:788`), `ordinary_channels_route_default` (`SpaceRefs.v:929`) | `prop_route_rules`, `reducer_space_routing_ordinary_unchanged` |
| HM-1, HM-2 | `canon_respects_par_eq` (`Namespace.v:1043`) | `prop_canonical_inner_bytes_respects_par_eq` |
| HM-4 | `name_id_routes_to_sid` (`Namespace.v:1003`) | `prop_name_id_routes_to_sid`, `space_handle_methods_gensym` |
| OC-1, OC-3 | `queue_ord_fresh` (`HeadOnlyDisciplines.v:1046`) | `prop_queue_ord_fresh`, `space_queue_counter_bump_shape` |
| OC-2 | `first_ops_race_on_space_counter` (`CounterConflict.v:939`) | `prop_first_ops_race_on_space_counter`, `space_counter_first_operation_bumps_the_space_counter` |
| OC-4 | None | `space_counter_rejects_corrupt_counters` |
| OC-5 | `ordered_writes_race_on_counter` (`CounterConflict.v:910`), `ordered_reads_race_on_counter` (`CounterConflict.v:924`), `same_block_ops_chain` (`CounterConflict.v:1113`), `shared_collection_race` (`CounterConflict.v:889`) | `space_queue_counter_race_conflict_enq_deq`, `space_queue_counter_race_conflict_same_block_one_chain`, `space_collection_conflicts_cell_writes_race`, `space_collection_conflicts_stack_push_pop_race`, `prop_cell_ops_race_on_counter` |
| OC-6 | `merge_without_race_is_serializable` (`CounterConflict.v:1489`), `crossed_stacks_without_removal_counters_fabricate` (`CounterConflict.v:2578`), the TLA+ invariant `NoMergedOrderFabrication` (U4, and V16 with `MC_CounterConflict_Stacks.cfg`) | `prop_nonracing_queue_branches_serializable`, `space_queue_counter_race_conflict_unrelated_queues_merge`, `prop_merged_stack_state_is_serial`, `space_collection_conflicts_crossed_stacks` |
| QD-1 | `queue_head_is_min_ord` (`HeadOnlyDisciplines.v:530`) | `prop_queue_candidates_head_only` |
| QD-2, QD-3 | `queue_head_blocks` (`HeadOnlyDisciplines.v:1218`), the TLA+ invariant `HeadBlocks` (U2) | `space_queue_head_only_data_head_blocks`, `space_queue_head_only_waiter_head_blocks` |
| QD-4, QD-5 | `queue_loop_bounded_or_reissues` (`HeadOnlyDisciplines.v:1380`) | `space_queue_reissue_persistent_send`, `space_queue_reissue_peek_produce`, `space_queue_divergent_pair_exhausts_phlo` |
| QD-6 | `queue_produce_preserves_inv` (`HeadOnlyDisciplines.v:928`), `queue_consume_preserves_inv` (`HeadOnlyDisciplines.v:948`), the TLA+ invariants `QuiescentNoPendingMatch` (U1) and `ShadowedNoPendingMatch` (U9), `queue_reentry_preserves_shadow_inv` (`HeadOnlyDisciplines.v:2712`), `queue_quiescent_inv` (`HeadOnlyDisciplines.v:2781`) | `prop_queue_produce_preserves_invariant`, `prop_queue_consume_preserves_invariant`, `prop_queue_shadow_invariant_with_inflight` |
| QD-7 | `queue_fifo` (`HeadOnlyDisciplines.v:1135`), `queue_fifo_with_reentries` (`HeadOnlyDisciplines.v:2946`), the TLA+ invariants `FifoOrder` (U3) and `LeaveOrder` (U12) | `space_queue_head_only_fifo_delivery`, `prop_queue_fifo_with_reentries` |
| QD-8 | None | `space_joins_queue_programs_run_as_joins`, `space_queue_send_params_rejected`, `space_queue_query_rejected` |
| QD-9 | `queue_reissue_keeps_ord` (`HeadOnlyDisciplines.v:2373`), `queue_head_blocks_reentry` (`HeadOnlyDisciplines.v:3021`), `persistent_reentry_never_spent` (`HeadOnlyDisciplines.v:2342`), the TLA+ invariants `ReissueKeepsOrd` (U10), `IntroducedOnce` (U11) and `FifoOrder` (U3) | `space_queue_reissue_reenters_ahead_of_younger_records`, `prop_queue_reissue_keeps_first_round_ord`, `prop_queue_reentry_head_blocks` |
| QF-1, QF-4 | `qualifier_matrix` (`SpaceFactory.v:383`) | `prop_qualifier_matrix`, `space_urn_factory_qualifier_support_matrix` |
| QF-2 | The TLA+ invariant `DurableSurvivesRestart` (U7), `view_charge_cache_independent` (`EventCorrelation.v:8773`) | `prop_handle_survives_checkpoint_reset`, `reified_handle_persisted_restart_replay`, `prop_attach_warm_cold_cost_equal`, `space_lazy_attach`, `space_checkpoint_reset`, `space_attach_warm_cold_cost_equal`, `space_restart_rehydrate_three_processes` |
| QF-3 | The TLA+ invariant `HandleCallsMergeLikeContracts` (U8) | `space_intents_for_two_sids_in_one_frontier_run_in_separate_tasks`, `reified_handle_cross_block_merge_no_conflict` |
| QF-5, QF-6 | `qualifier_matrix` (`SpaceFactory.v:383`), `ephemeral_durable_disjoint` (`Namespace.v:867`) | `reducer_qualifier_matrix`, `space_temp_replay_equal_hashmap` to `space_temp_replay_equal_vector`, `reified_factory_ephemeral_staging`, `pattern_enforcement_temp_rows` |
| QF-7 | The P1 headlines of the substrate | `reified_ephemeral_cost_charged` |
| QF-8 | `sweep_complete` (`VolatileSweep.v:1714`), `sweep_exact` (`VolatileSweep.v:894`), `sweep_root_neutral` (`VolatileSweep.v:1266`) | `prop_sweep_complete`, `space_temp_swept_at_deploy_end`, `space_temp_join_swept`, `space_temp_failed_deploy_reverts`, `space_temp_many_volatile_keys_export`, `reified_sweep_runs_at_each_entry_point_of_the_reducer`, `reified_sweep_runs_at_each_entry_point_native` |
| QF-9 | `dead_ephemeral_dangling` (`VolatileSweep.v:1759`) | `prop_dead_ephemeral_dangling`, `reified_ephemeral_dangling_next_deploy`, `reified_ephemeral_dangling_charge_is_fixed`, `reducer_dangling_handle` |
| QF-10 | The gate 9 model `check_mirror` of `SpaceFactory.v` | `mirror_temp_never_mirrored`, `reducer_qualifier_matrix`, `pattern_enforcement_native_unsupported_pairs_reject` |
| QF-11 | `qualifier_matrix` (`SpaceFactory.v:383`), the TLA+ invariant `QualifierSupport` (V6) | `prop_seq_qualifier_support`, `reducer_qualifier_matrix`, `pattern_enforcement_qualifiers_fail_closed`, `pattern_enforcement_seq_rows` |
| QF-13, QF-14 | `seq_name_iff` (`Namespace.v:1150`), the TLA+ invariant `SeqNotSent` (V5) | `prop_seq_scan_finds_every_seq_name`, `reducer_seq_payload` |
| QF-15, QF-16, QF-17 | The TLA+ invariants `SeqExclusive` (V4) and `ClaimTracksLastUse` (V8) | `prop_seq_claim_chain`, `reducer_seq_atomic_guard`, `pattern_enforcement_seq_rows` |
| QF-18 | The TLA+ invariant `SeqExclusive` (V4) | `reducer_seq_atomic_guard` |
| FA-1 | `registered_families_complete` (`SpaceFactory.v:1444`) | `prop_registered_families_complete`, `space_factory_families_registered` |
| FA-2 | `space_match_is_injected_match` (`Match.v:642`) | `space_families_dispatch_by_config_reserves_one_comparison_per_arm`, `space_family_dispatch_by_config` |
| FA-3 | `aliases_bind_family_capability` (`SpaceFactory.v:1533`) | `prop_aliases_bind_family_capability`, `reified_getspaceagent_alias_equal` |
| FA-4, FA-6 | `constructor_params_validated` (`SpaceFactory.v:1938`) | `prop_constructor_params_validated`, `space_factory_kwargs_unknown_param_with_suggestion` |
| FA-5 | `priorities_validated` (`SpaceFactory.v:2075`), `array_size_validated` (`SpaceFactory.v:2188`) | `prop_priorities_validated`, `prop_array_size_validated` |
| FA-7 | `array_size_validated` (`SpaceFactory.v:2188`) | `space_factory_kwargs_are_stored_canonical` |
| TH-1 | `builtin_decidable_structural` (`TheoryAdmission.v:254`) | `prop_theory_builtin_decidable`, `theory_nat_accepts_bigint_nonneg`, `theory_nat_rejects_minus1_string_par` |
| TH-2, TH-2a | `identity_bytes_roundtrip` (`TheoryAdmission.v:822`) | `prop_theory_identity_bytes_roundtrip`, `theory_identity_persisted_no_handle`, `theory_config_slot_is_one_compact_par` |
| TH-3, TH-7 | `resolve_identity_deterministic` (`TheoryAdmission.v:587`), `replay_preserves_theory_admission` (`TheoryAdmission.v:851`) | `prop_resolve_identity_deterministic`, `theory_unavailable_fails_each_config_read`, `reified_factory_theory_argument` |
| TH-4 | `theory_gate_validated` (`SpaceFactory.v:2525`) | `prop_theory_gate`, `theory_unknown_name_suggestion`, `theory_builtin_args_rejected`, `reified_factory_theory_gate` |
| TH-5 | `fail_closed` (`TheoryAdmission.v:398`), `replay_preserves_theory_admission` (`TheoryAdmission.v:851`) | `prop_theory_fail_closed`, `prop_replay_preserves_theory_admission`, `theory_refuted_value_aborts_before_staging`, `theory_undetermined_budget0_aborts`, `theory_admission_precedes_any_staging` |
| TH-6 | `theory_refusal_is_guard_refusal` (`Match.v:795`) | `prop_theory_refusal_is_guard_refusal`, `space_guard_match`, `space_guard_match_refusal_is_a_guard_refusal`, `space_theory_admission`, `space_theory_admission_vectordb` |
| TH-8 | `usage_cache_transparent` (`TheoryAdmission.v:475`), `builtin_decidable_structural` (`TheoryAdmission.v:254`) | `prop_theory_usage_cache_transparent`, `theory_admission_reserves_one_unit_per_checked_value`, `theory_match_reserves_one_unit_per_stored_datum` |
| TH-9 | `fail_closed` (`TheoryAdmission.v:398`) | `theory_nested_commit_guard` |
| TH-10 | None | `theory_nested_commit_guard`, `space_guard_match_refusal_is_a_guard_refusal`, `theory_undetermined_budget0_aborts` |
| AR-1, AR-3, VE-1, VE-3 | `fixed_array_allocates_at_most_size` (`OuterStorage.v:313`), `vector_index_is_counter` (`VectorStore.v:184`) | `prop_fixed_array_allocates_at_most_size`, `prop_vector_index_is_counter` |
| AR-2, VE-2 | `hashmap_get_put_other` (`HashMapStore.v:256`), `hashmap_iteration_permutation_invariant` (`HashMapStore.v:211`) | `prop_hashmap_get_put_other`, `prop_hashmap_iteration_permutation_invariant` |
| AR-4, VE-4, AL-2 | `fixed_array_indices_sequential` (`OuterStorage.v:345`), `cyclic_array_index_mod_size` (`OuterStorage.v:376`), `vector_gensym_monotonic` (`VectorStore.v:219`) | `prop_fixed_array_indices_sequential`, `prop_cyclic_array_index_mod_size`, `prop_vector_gensym_monotonic` |
| AL-3 | `allocation_batch_atomic` (`OuterStorage.v:402`), `fixed_array_out_of_names_exact` (`OuterStorage.v:330`) | `prop_allocation_batch_atomic`, `prop_fixed_array_out_of_names_exact` |
| AL-4 | `allocator_identities_unique` (`OuterStorage.v:555`) | `prop_allocator_identities_unique` |
| AL-5 | The TLA+ invariants `NoDuplicateIndex` (V9) and `AllocationsRace` (V10) | `prop_no_duplicate_index`, `prop_allocations_race`, `space_allocator_race_conflict` |
| AL-7 | `alloc_trace_covered` (`OuterStorage.v:695`) | `prop_alloc_trace_covered` |
| AL-6, AL-8 | None | `reducer_space_routing_allocation_jobs_give_index_names`, `reducer_space_routing_baseline_job_reserves_its_reads` |
| PP-1, PP-2 | `prefix_key_iff_prefix` (`PathMapPrefix.v:584`), `candidate_order_total` (`PathMapPrefix.v:1014`) | `prop_prefix_key_iff_prefix`, `prop_candidate_order_total` |
| PP-3, PP-4 | None | `the_depth_cap_bounds_channel_paths_and_prefixes`, `pathmaps_and_zippers_are_unsupported_at_any_depth` |
| PP-5 | `reroot_ident` (`PathMapPrefix.v:843`), `reroot_compose` (`PathMapPrefix.v:850`), `fips_example` (`PathMapPrefix.v:959`) | `prop_reroot_ident`, `prop_reroot_compose`, `prop_unit_shift`, `fips_example` |
| PP-6 | `reroot_injective` (`PathMapPrefix.v:922`) | `prop_head_adapter_roundtrip`, `prop_reroot_injective` |
| PS-1 | `prefix_contact_anchored` (`PathMapPrefix.v:1948`) | `the_path_channels_are_storage_channels_of_the_space`, `the_token_is_the_space_counter_and_the_counter_of_every_path` |
| PS-2 | `path_element_rules` (`PathMapPrefix.v:1455`) | `prop_path_element_rules`, `check_key_follows_the_path_rules`, `gensym_names_root_a_private_subtree` |
| PS-3 | `marker_record_roundtrip` (`PathMapPrefix.v:1350`), `marker_invariant_preserved` (`PathMapPrefix.v:2560`), the TLA+ invariant `MarkerCount` (V24) | `prop_marker_record_roundtrip`, `prop_marker_invariant_preserved`, `marker_random_states_differ_by_depth_and_follow_the_path_marker_purpose` |
| PS-4 | `vehicle_carries_payload` (`PathMapPrefix.v:1382`), `delivery_vehicle_single_channel` (`PathMapPrefix.v:1911`) | `prop_vehicle_carries_payload`, `a_vehicle_is_the_marker_followed_by_the_payload` |
| PS-5 | `candidate_order_total` (`PathMapPrefix.v:1014`), `no_match_remains` (`PathMapPrefix.v:2380`) | `prop_candidates_cover_the_prefix_in_key_order`, `the_fips_paths_deliver_the_exact_path_first_and_rerooted` |
| PS-6 | `no_match_remains` (`PathMapPrefix.v:2380`), the TLA+ invariant `NoMatchRemains` (V23) | `prop_waiter_prefixes_are_most_specific_first`, `the_waiters_of_a_send_are_the_most_specific_first` |
| PS-7 | `delivery_vehicle_single_channel` (`PathMapPrefix.v:1911`), `marker_invariant_preserved` (`PathMapPrefix.v:2560`) | `a_removal_retires_the_record_and_every_other_marker`, `prop_marker_invariant_preserved` |
| PS-8 | `peek_restores_delivered_marker` (`PathMapPrefix.v:2010`) | `prop_marker_invariant_preserved` |
| PS-9 | `token_ord_reentry` (`PathMapPrefix.v:3126`) | `the_token_is_the_space_counter_and_the_counter_of_every_path` |
| PS-10 | `prefix_contact_anchored` (`PathMapPrefix.v:1948`), the TLA+ invariant `MergeDetectsPrefixContact` (V25) | `the_path_channels_are_storage_channels_of_the_space` |
| HS-1 | `hashmap_get_put_other` (`HashMapStore.v:256`) | `space_hashset_keys_channels_and_names_are_those_of_the_hashmap_store` |
| HS-2, HS-3 | `hashset_unit_rules` (`HashMapStore.v:378`) | `prop_hashset_unit_rules`, `reified_hashset_payload_not_empty`, `reified_hashset_pattern_not_empty`, `reified_hashset_guards`, `space_engine_hashset_families_take_units_only`, `space_hashset_accepts_exactly_the_empty_payload`, `space_hashset_accepts_exactly_the_empty_pattern_list`, `pattern_enforcement_hashset_rows` |
| HS-4 | None | `reified_hashset_joins`, `reified_hashset_pattern_not_empty` |
| HS-5 | `hashset_unit_rules` (`HashMapStore.v:378`) | `reified_hashset_unit_sends`, `reified_hashset_disciplines`, `reified_hashset_head_order`, `space_engine_hashset_families_take_units_only` |
| HS-6 | `qualifier_matrix` (`SpaceFactory.v:383`), the TLA+ invariant `QualifierSupport` (V6) | `reified_hashset_seq_only`, `reified_hashset_absent_unit_waits`, `reducer_qualifier_matrix`, `reified_factory_hashset_takes_only_seq_in_native_execution`, `pattern_enforcement_native_unsupported_pairs_reject` |
| HS-7 | `registered_families_complete` (`SpaceFactory.v:1444`) | `reified_hashset_factory_params`, `space_factory_families_registered`, `p7_system_channels_have_the_table_values` |
| BG-1 | `bag_visit_order_canonical` (`DataCollection.v:243`), `waiter_visit_order_canonical` (`ContinuationCollection.v:337`) | `prop_bag_visit_order_canonical`, `prop_bag_waiter_visit_order_canonical` |
| BG-2, BG-3 | `bag_consume_first_spatial_match` (`DataCollection.v:445`), `guard_once_greedy_like_head` (`Match.v:685`) | `prop_bag_consume_first_spatial_match`, `prop_engine_guard_checked_once_as_the_host_checks_it` |
| BG-4 | `bag_multiplicity_preserved` (`DataCollection.v:559`), `bag_removal_keeps_equal_duplicate` (`DataCollection.v:578`), `bag_persistent_match_not_removed` (`DataCollection.v:536`) | `prop_bag_multiplicity_preserved`, `prop_bag_removal_keeps_equal_duplicate` |
| BG-5, BG-6 | `bag_peek_restore_is_host_second_send` (`DataCollection.v:725`), `peek_restore_equals_host_reproduce` (`GenericRSpace.v:1156`), the TLA+ invariant `PeekRestoresRecord` (V8) | `prop_bag_legacy_charges_equal_host`, `prop_bag_restore_registers_the_record_authority` |
| BG-7 | `scan_reissue_identity_stable` (`GenericRSpace.v:1300`) | `prop_engine_scan_reissue_keeps_identity` |
| BG-8 | `scan_trace_covered` (`DataCollection.v:856`) | `prop_bag_reservation_cut_rejects_without_effect` |
| CL-1 | `cell_capacity_one` (`CellDiscipline.v:672`), the TLA+ invariant `CellCapacity` (V7) | `prop_cell_capacity_one`, `space_disciplines_cell_second_produce_occupied` |
| CL-2, CL-3 | `cell_capacity_one` (`CellDiscipline.v:672`) | `space_disciplines_cell_every_operation_bumps_counter`, `space_disciplines_cell_branch_writes_race_on_the_counter` |
| CL-4 | `cell_restore_never_occupied` (`CellDiscipline.v:858`) | `prop_cell_restore_never_occupied` |
| CL-5 | `cell_reentry_as_fresh` (`CellDiscipline.v:889`) | `prop_cell_reentry_as_fresh` |
| RQ-1 to RQ-3 | `query_source_routes_inner_channel` (`SpaceRefs.v:1617`) | `prop_query_bind_routes_inner_channel`, `prop_query_source_routes_inner_channel` |
| SK-1 | `stack_head_is_max_ord` (`StackDiscipline.v:556`), `stack_head_blocks` (`StackDiscipline.v:1348`) | `prop_stack_head_is_max_ord` |
| SK-2 | `stack_reentry_at_lifo_place` (`StackDiscipline.v:1444`), `stack_reissue_keeps_ord` (`StackDiscipline.v:1576`), the TLA+ invariant `ReissueKeepsOrd` (V14) | `prop_stack_reissue_keeps_ord_any_schedule` |
| SK-3, SK-4 | `stack_produce_preserves_inv` (`StackDiscipline.v:1043`), `stack_settle_keeps_unspent_records` (`StackDiscipline.v:2151`), `stack_peek_restore_needs_no_settle` (`StackDiscipline.v:1405`), the TLA+ invariants `StackNoPendingMatch` (V12) and `NoRecordLost` (V13) | `prop_stack_no_pending_match_any_schedule` |
| SK-5, SK-6 | `stack_settle_terminates` (`StackDiscipline.v:1959`), the TLA+ invariant `TxnFiresBounded` (V15) | `prop_stack_txn_fires_bounded_at_the_cap`, `prop_stack_settle_bounded_any_schedule`, `space_stack_plan_after_a_full_settle_reissues_the_operation` |
| SK-7 | `stack_lifo` (`StackDiscipline.v:1255`), the TLA+ invariant `LifoOrder` (V11) | `prop_stack_lifo_order_any_schedule` |
| ST-1, ST-2 | `set_key_injective` (`SetDiscipline.v:297`) | `prop_set_key_injective`, `space_set_discipline_key_length_prefix_no_collision` |
| ST-3 | `set_view_dedups_equal_pars` (`SetDiscipline.v:704`) | `prop_set_view_dedups_equal_pars`, `space_set_discipline_duplicates_stay_distinct_records` |
| ST-4, ST-5 | `set_consume_retires_class` (`SetDiscipline.v:792`), `set_retire_comm_lines_up` (`SetDiscipline.v:872`) | `prop_set_consume_retires_class`, `prop_set_retire_comm_lines_up` |
| ST-6 | `set_produce_fire_retires_stored_class` (`SetDiscipline.v:1151`), the TLA+ invariant `SetClassRetired` (V6) | `prop_set_produce_fire_retires_stored_class`, `prop_set_class_retired_any_schedule`, `space_set_discipline_guarded_waiter_then_equal_send_retires_class` |
| ST-7 | `set_peek_restores_representative_only` (`SetDiscipline.v:1205`) | `prop_set_peek_restores_representative_only`, `space_set_discipline_peek_restore_reoffer_retires_class` |
| ST-8 | `set_membership_full_compare_on_equal_hash` (`SetDiscipline.v:1274`) | `prop_set_membership_full_compare_on_equal_hash` |
| ST-9 | `scan_reissue_identity_stable` (`GenericRSpace.v:1300`), `set_retirements_race` (`CounterConflict.v:1992`), `unordered_set_writes_commute` (`CounterConflict.v:2024`) | `space_set_discipline_persistent_reissue_keeps_identity`, `space_set_discipline_equal_receives_stay_two_waiters`, `prop_set_retirements_race`, `prop_unordered_set_writes_commute`, `space_collection_conflicts_set_retirements_race`, `space_collection_conflicts_set_writes_merge` |
| ST-10 | `set_key_trace_covered` (`SetDiscipline.v:1389`) | `prop_set_key_trace_covered`, `space_set_discipline_failed_canonicalization_rejects_before_staging` |
| PQ-1 | `pq_level_order` (`PriorityQueue.v:863`), `pq_unprioritized_lowest` (`PriorityQueue.v:887`), the TLA+ invariant `PqLevelOrder` (V17) | `prop_pq_level_order`, `space_disciplines_pq_ascending_levels`, `space_disciplines_pq_unprioritized_lowest` |
| PQ-2, PQ-4 | `pq_restricted_waiter_in_level_queue` (`PriorityQueue.v:1921`), `pq_unrestricted_first_matching_level` (`PriorityQueue.v:1943`), `pq_fifo_within_level` (`PriorityQueue.v:2151`), `pq_head_blocks_within_level` (`PriorityQueue.v:2324`) | `prop_pq_fifo_order_any_schedule`, `space_disciplines_pq_restricted_waiter_waits_in_level_queue`, `space_disciplines_pq_head_blocks_within_level` |
| PQ-3 | `pq_no_level_storage` (`PriorityQueue.v:3903`), `datum_priority_slot_roundtrip` (`RecordEnvelope.v:1506`), `continuation_policy_slot_roundtrip` (`RecordEnvelope.v:1542`) | `prop_pq_no_level_storage`, `prop_pq_record_slots_roundtrip` |
| PQ-5, PQ-8 | `pq_settle_oldest_first` (`PriorityQueue.v:1899`), `pq_settle_order_matters` (`PriorityQueue.v:4081`), `pq_produce_preserves_inv` (`PriorityQueue.v:1390`), `pq_without_settle_breaks_inv` (`PriorityQueue.v:4002`), the TLA+ invariants `PqOldestFirst` (V18) and `PqNoPendingMatch` (V19) | `prop_pq_oldest_first_any_schedule`, `prop_pq_no_pending_match_any_schedule`, `space_disciplines_pq_older_waiter_wins` |
| PQ-6 | `pq_consume_preserves_inv` (`PriorityQueue.v:1404`), `pq_unrestricted_first_matching_level` (`PriorityQueue.v:1943`) | `prop_pq_no_pending_match_any_schedule`, `space_disciplines_pq_guarded_unrestricted_receive_parks` |
| PQ-7 | `pq_peek_restores_head` (`PriorityQueue.v:2486`), `pq_settle_keeps_unspent_records` (`PriorityQueue.v:2864`), the TLA+ invariant `NoRecordLost` (V20) | `space_disciplines_pq_peek_restores_head`, `space_disciplines_pq_produce_peek_waiter_reoffers` |
| PQ-9 | `pq_priority_domain` (`PriorityQueue.v:3730`) | `prop_pq_priority_domain`, `space_disciplines_pq_priorities_bound_rejects`, `space_disciplines_pq_priority_param_type_checked` |
| PQ-10 | `pq_settle_terminates` (`PriorityQueue.v:3181`), `pq_reissue_takes_level_fifo_place` (`PriorityQueue.v:2673`), `pq_reentered_waiter_keeps_seniority` (`PriorityQueue.v:2716`), the TLA+ invariants `TxnFiresBounded` (V22) and `ReissueKeepsOrd` (V21) | `prop_pq_txn_fires_bounded_at_the_cap`, `prop_pq_reissue_keeps_ord_any_schedule` |
| JN-1 | None | `space_joins_cross_space_join_aborts`, `space_joins_default_and_space_join_aborts`, `reducer_space_routing_agent_gate` |
| JN-2 | `plan_then_lock_stable` (`JoinCascade.v:5723`) | `prop_plan_then_lock_covers_every_staged_channel`, `space_joins_escalation_relocks_once`, `space_joins_classification_reserves_check_joins` |
| JN-3 | `join_repeated_channel_distinct_records` (`JoinCascade.v:1244`), `bind_permutation_restores_order` (`JoinCascade.v:2148`), `continuation_v2_roundtrip` (`RecordEnvelope.v:1734`), the TLA+ invariant `DistinctRecordsPerBind` (V28) | `prop_join_repeated_channel_distinct_records`, `prop_bind_permutation_restores_order`, `prop_continuation_v2_roundtrip`, `space_joins_repeated_channel_distinct_records`, `space_joins_bind_order_restored_for_body`, `space_joins_multi_ord_envelope_roundtrip` |
| JN-4 | None | `space_joins_peek_join_restores_stored_candidates`, `space_joins_reissued_join_keeps_bind_ords` |
| JN-5 | `cascade_bumps_touched_counters` (`JoinCascade.v:4056`), `join_reissue_keeps_bind_ords` (`JoinCascade.v:4130`), the TLA+ invariants `TouchedCountersBumped` (V34) and `ReissueKeepsBindOrds` (V33) | `prop_cascade_bumps_touched_counters`, `prop_join_reissue_keeps_bind_ords`, `space_joins_multi_ord_envelope_roundtrip`, `space_joins_reissued_join_keeps_bind_ords` |
| JN-6, JN-7 | `join_fires_heads_only` (`JoinCascade.v:3210`), the TLA+ invariant `JoinFiresHeadsOnly` (V27) | `prop_join_fires_heads_only`, `space_joins_queue_heads_only`, `space_joins_stack_heads_only`, `space_joins_pq_heads_only`, `space_joins_cell_join` |
| JN-8 | `join_search_is_head_greedy` (`JoinCascade.v:1468`), `greedy_join_can_miss_assignment` (`JoinCascade.v:1345`) | `prop_join_search_is_head_greedy`, `prop_greedy_join_limit`, `space_joins_bag_greedy`, `space_joins_set_classes`, `space_joins_guard_checked_once_after_all_binds` |
| JN-9 | `join_produce_search_agrees_with_head` (`JoinCascade.v:1871`), `join_incoming_binds_once` (`JoinCascade.v:1146`), the TLA+ invariant `IncomingBoundOnce` (V29) | `prop_join_produce_search_agrees_with_head`, `prop_join_incoming_binds_once`, `space_joins_incoming_binds_once` |
| JN-10 | `cascade_terminates` (`JoinCascade.v:4497`), `cascade_order_independent` (`JoinCascade.v:3144`), `lift_reoffer_equals_direct_comm` (`JoinCascade.v:3576`), the TLA+ invariant `CascadeMeasureDecreases` (V30) | `prop_cascade_terminates_any_schedule`, `prop_cascade_order_independent`, `prop_lift_reoffer_equals_direct_comm`, `prop_cascade_measure_decreases`, `space_joins_cascade_after_produce`, `space_joins_cascade_after_consume`, `space_joins_cascade_order_independent`, `space_joins_set_join_cascade_fires_exposed_waiter` |
| JN-11 | `join_set_bind_retires_class` (`JoinCascade.v:2973`) | `prop_join_set_bind_retires_class`, `space_joins_set_bind_retires_class` |
| JN-12 | `peek_join_restores_stored_candidates` (`JoinCascade.v:5156`), the TLA+ invariant `NoRecordLost` (V32) | `prop_peek_join_restores_stored_candidates`, `prop_no_record_lost_any_schedule`, `space_joins_peek_join_restores_stored_candidates`, `space_joins_produce_side_peek_reproduces_in_receipt_order` |
| JN-13 | `join_txn_fires_bounded` (`JoinCascade.v:4642`), the TLA+ invariant `TxnFiresBounded` (V31) | `prop_join_txn_fires_bounded_at_the_cap`, `space_joins_txn_fires_bounded_at_the_cap`, `space_joins_cascade_round_cap_reissues_settle`, `space_joins_cap_carries_pending_second_sends`, `space_joins_settle_before_cap_resumes_the_operation` |
| JN-14 | `cascade_restores_inv` (`JoinCascade.v:7230`), the TLA+ invariant `QuiescentNoPendingMatch` (V26) | `prop_cascade_restores_inv_any_schedule`, `prop_quiescent_no_pending_match_any_schedule`, `prop_join_engine_agrees_with_drivers_without_joins` |
| JN-15 | `join_ops_race_on_every_counter` (`CounterConflict.v:1947`) | `prop_join_ops_race_on_every_counter`, `space_collection_conflicts_join_ops_race_all_counters`, `space_collection_conflicts_unrelated_collections_merge`, `space_collection_conflicts_bag_join_potential_comm`, `space_joins_persisted_waiter_check3_conflict`, `reified_p5c_only_user_joins_persist` |
| NE-1, NE-3 | `native_replay_reproduces_play` (`EventCorrelation.v:8617`), `native_replay_denial_keeps_prefix` (`EventCorrelation.v:8663`), the TLA+ invariant `NativeReplayOutcomeEqual` (N5) | `prop_native_space_replay_reproduces_play`, `prop_native_space_replay_denial_keeps_prefix`, `space_native_replay_equal_hashmap`, `space_native_denial_replay`, `reducer_space_costs` |
| NE-2 | `view_charge_cache_independent` (`EventCorrelation.v:8773`) | `prop_view_charge_cache_independent`, `native_space_replay_view_charges_equal_play`, `prop_attach_warm_cold_cost_equal`, `space_attach_cost`, `space_attach_warm_cold_cost_equal`, `reducer_attach_costs` |
| NE-4 | `native_replay_fails_closed` (`EventCorrelation.v:8708`), the TLA+ invariant `NoDeadlock` (N7) | `prop_native_space_replay_fails_closed`, `space_native_forged_predecessor_fails_closed`, `space_native_forged_footprints_never_hang` |
| NE-5 | The TLA+ invariant `NoBlockedReadiness` (N6) | `prop_space_key_keeps_one_space_in_one_group`, `space_native_space_key_groups` |
| VD-1 | `vectordb_params_validated` (`SpaceFactory.v:2387`) | `prop_vectordb_params_validated`, `reified_factory_gate_9_needs_the_default_qualifier_and_an_indexed_metric` |
| VD-2 | None | `space_vectordb_payload_is_the_vector_by_default`, `space_vectordb_peek_restores_records_with_their_embedding`, `space_native_vectors_every_kind_enters_exactly` |
| VD-3 | None | `space_vectordb_errors_have_their_codes`, `space_vectordb_source_errors_have_their_codes`, `space_vectordb_fixed_point_and_rational_thresholds` |
| VD-4 | `cosine_squared_form_iff` (`Similarity.v:412`), `cosine_order_squared_form` (`Similarity.v:471`), `euclidean_radius_squared_form` (`Similarity.v:545`), `binary_laws` (`Similarity.v:1042`), `selection_total_order` (`Similarity.v:1169`) | `prop_selection_order_is_a_strict_total_order`, `space_vectordb_bounds_select_the_eligible_records`, `space_vectordb_ids_keep_only_the_listed_records`, `space_vectordb_binary_metrics_read_binary_vectors_only`, `space_vectordb_reads_without_a_vector_follow_the_canonical_order`, `space_similarity_topk_reference_cases_through_the_space`, `space_hash_collision_tie_duplicates_are_interchangeable`, `space_hash_collision_tie_forced_equal_source_hashes_select_one_value` |
| VD-5 | `topk_deterministic` (`Similarity.v:1199`) | `prop_topk_is_deterministic`, `space_vectordb_single_mode_takes_the_best_record`, `space_vectordb_guard_refusal_parks_until_a_worse_record_passes`, `space_vector_discipline_restart_keeps_values_ties_and_order` |
| VD-6 | `list_selection_deterministic` (`Similarity.v:1278`) | `prop_vectordb_list_is_ordered_and_complete`, `space_vectordb_list_mode_delivers_one_list_in_selection_order`, `rspace_list_random_state_zero_one_and_two_members`, `rspace_list_random_state_replay_reproduces_the_merged_states` |
| VD-7 | the TLA+ invariant `ListAtomic` (W2) | `prop_vectordb_list_fires_are_atomic`, `space_vectordb_ce1_list_waiter_fires_with_the_incoming_datum` |
| VD-8 | `vdb_txn_bounded` (`VectorDbDiscipline.v:924`), the TLA+ invariant `CascadeTerminates` (W4) | `prop_vectordb_transactions_are_bounded`, `space_vectordb_list_cascade_carries_at_the_cap` |
| VD-9 | `vdb_no_record_lost` (`VectorDbDiscipline.v:1082`), the TLA+ invariant `PeekConservation` (W3) | `prop_vectordb_records_are_conserved`, `space_vectordb_cascade_peek_restores_the_lifted_member` |
| VD-10 | `vdb_no_pending_match` (`VectorDbDiscipline.v:662`), `vdb_list_needs_cascade` (`VectorDbDiscipline.v:1311`), the TLA+ invariant `QuiescentNoPendingMatch` (U1) | `prop_vectordb_no_pending_match`, `space_vectordb_ce2_removal_cascade_fires_the_list_waiter` |
| VD-11 | `vdb_join_removal_exposes_nothing` (`VectorDbDiscipline.v:781`) | `prop_vectordb_driver_and_engine_agree`, `space_vectordb_join_binds_each_position_by_its_query`, `space_vectordb_list_mode_in_a_join_fails`, `space_vectordb_join_waiter_fires_on_an_eligible_send` |
| VD-12 | `capped_writes_soft_bound` (`CounterConflict.v:2383`) | `prop_vectordb_capacity_soft_bound`, `space_collection_conflicts_vectordb_capacity_soft_bound`, `prop_vectordb_writes_commute`, `space_vectordb_capacity_bounds_records_not_waiters` |
| VD-13 | `squared_forms_embed` (`Similarity.v:1000`), `root_midpoint_nearest` (`VectorOps.v:367`) | `prop_dyadic_and_rational_kernels_agree`, `space_vectordb_scores_are_the_correctly_rounded_exact_scores`, `tensor_square_roots_are_correctly_rounded`, `space_similarity_topk_reference_cases_through_the_space` |
| VD-14 | None | `space_native_vectordb_errors_reproduce`, `space_native_vectordb_cascade_carries_at_the_cap`, `space_native_vectordb_denial_replay`, `space_vectordb_reservation_cut_rejects_without_effect`, `reducer_query_costs_of_a_query_follow_its_records_and_dimensions`, `reducer_query_costs_follow_the_table_of_the_module` |
| SP-1 | None | `vector_ops_params_and_persistent_sends_abort`, `vectordb_embed_and_ann_reject_params_and_persistent_sends` |
| SP-2 | `vector_replies_exact` (`VectorOps.v:264`), `root_midpoint_nearest` (`VectorOps.v:367`), `dot_laws` (`Similarity.v:658`), `distance_laws` (`Similarity.v:745`) | `prop_vector_ops_agree_with_an_exact_oracle`, `prop_vector_ops_float_inputs_give_floats`, `prop_vector_ops_roots_are_correctly_rounded`, `vector_ops_failures_reply_their_codes` |
| SP-3 | None | `vectordb_embed_replies_and_codes` |
| SP-4 | the TLA+ invariant `EqualHostWork` (W7) | `vectordb_ann_replies_and_codes`, `prop_vectordb_services_replay_reproduces_the_recorded_outputs` |
| SP-5 | the TLA+ invariant `ConsensusIndependentOfMirror` (W5), the TLA+ invariant `AdvisoryRecorded` (W6) | `vectordb_embed_replay_equal`, `vectordb_ann_replay_equal`, `prop_vectordb_services_replay_reproduces_the_recorded_outputs` |
| SP-6 | None | `vectordb_config_reports_the_mirror` |
| MR-1 | the TLA+ invariant `NetWritesExact` (W9) | `prop_mirror_capture_net_writes_are_the_change`, `mirror_capture_net_writes_follow_the_first_and_the_last_event`, `mirror_rule_e_coalesces` |
| MR-2 | the TLA+ invariant `PublishOnlyOnSuccess` (W8) | `prop_mirror_journal_keeps_the_last_write`, `mirror_exploratory_evaluation_publishes_nothing`, `mirror_journal_keeps_the_last_write_of_a_deploy` |
| MR-3 | the TLA+ invariant `PublishOnlyOnSuccess` (W8) | `prop_mirror_tracks_the_published_deploys`, `mirror_journal_drops_failed_deploy`, `mirror_replay_publishes_its_own_journal` |
| MR-4 | the TLA+ invariant `PublishNeverWaits` (W10) | `prop_mirror_writer_applies_the_accepted_batches`, `mirror_never_blocks_deploy` |
| MR-5 | the TLA+ invariant `ReconciledEqualsFinalizedView` (W11), the TLA+ invariant `WatermarkHonest` (W12) | `prop_mirror_reconcile_repairs_the_finalized_view`, `prop_mirror_reconciler_watermarks_follow_finished_repairs`, `mirror_reconcile_removes_orphans`, `mirror_reconcile_repairs_a_dirty_collection` |
| MR-6 | None | `mirror_restart_rescans_reports`, `mirror_reconciler_follows_the_finalized_blocks` |
| MR-7 | None | `vectordb_services_default_to_unavailable` |

No model states the rules OC-4, QD-8, AL-6, AL-8, PP-3, PP-4 and JN-1, because they are checks of limits, of input or of fail-closed rules.
Their tests show each abort.
Rule JN-4 holds by construction, because a normalized receive carries one `persistent` flag and one `peek` flag for all its binds.

## 8. Native execution

Protocol version 6 runs user deploys in native execution ([decision 47](decision-register.md#decision-47)).
[Native play](glossary.md#native-play) runs a deploy with the native phlo budget and records a [journal row](glossary.md#journal-row) for each operation.
[Native replay](glossary.md#native-replay) runs the deploy again against the journal, as proposer self-certification and validator replay do.
The rules of this section apply to every space operation in both modes.

**Rule NE-1: native play.** A space transaction of native play stages and commits its operations as in legacy play.
It reads its view through [metered view reads](glossary.md#metered-view-read), and native play records one journal row for each staged operation.
A space error is a deterministic `ReduceError`, as in legacy play.

**Rule NE-2: view charges.** A metered view load charges its key and the value that it loads.
The charge is the same on a cache hit and on a miss, so it never depends on cache state ([I-12](decision-register.md#i-12)).

**Rule NE-3: native replay reproduces native play.** Native replay stages the same operations, and the [native backend](glossary.md#native-backend) replays each one against its row.
It makes the observer calls of play in the order of play, applies the effects of play, and ends with the status of play.
A recorded denial ends the transaction with OutOfPhlogistons, and the operations before it stay applied.

**Rule NE-4: fail closed.** The native backend never waits for a row.
These four cases each fail the operation closed:

- a row that is not ready
- a footprint that differs from the footprint of the row
- an outcome that does not fit the staged kind
- a candidate that is not the staged COMM

The failed operation has no effect, and the failure closes the epoch of the session, so no operation waits for its row ([epoch closure](glossary.md#epoch-closure)).

**Rule NE-5: scheduling.** The transactions of one space run in one conflict group, in operation order, by the per-space key of [I-11](decision-register.md#i-11).
So every predecessor of a space row is complete when the row begins.

## 9. System processes of the VectorDB family

Three system processes serve the VectorDB family ([decision 57](decision-register.md#decision-57)).
Table 10 names them, and section [N16 of the design](design.md#n16-phase-p6-the-chromadb-search-backend) explains their code.

*Table 10. The system processes of the VectorDB family.*

| URN | Channel | Body reference | Call |
|---|---|---|---|
| `rho:lang:vector` | `[0xE0, 0x23]` | 235 | `vec!(ret, op, x…)` |
| `rho:vectordb:embed` | `[0xE0, 0x24]` | 236 | `embed!(ret, texts)` |
| `rho:vectordb:ann` | `[0xE0, 0x25]` | 237 | `ann!(ret, space, channel, query, options)` |

**Rule SP-1: calls and replies.** Each process takes the reply channel first, so `!?` applies.
It replies `[true, value…]`, or `[false, code, detail]` with a fixed detail text, so `try … catch` reads the reply.
A call with send params aborts the deploy with `ParamsUnsupported`, and a persistent call `!!` aborts it with `PersistentOpCall`.

**Rule SP-2: the operations of `rho:lang:vector`.** The call `vec!?(op, x…)` names one of five operations (`rholang/src/rust/interpreter/tensor/ops.rs:234-272`):

- `add(a, b)` and `scale(v, s)` reply a vector, and `dot(a, b)` replies a number.
- `norm(v)` and `distance(a, b)` reply the `f64` nearest to the root of an exact square, with ties to even.

`add`, `scale` and `dot` compute exactly, and they reply in the widest kind of their inputs ([decision 66](decision-register.md#decision-66)).
When every input is an integer or a Boolean, the reply is a `GInt`, or a `GBigInt` when the value does not fit.
When an input is a big rational or a fixed point and no input is a float, the reply is a `GBigRat`.
When an input is a float, the reply is a `GDouble`, the one rounding of the exact value.
So `dot` rounds its exact sum once, and never a partial sum ([decision 54](decision-register.md#decision-54), `vector_replies_exact`, S27 of `VectorOps.v`).
The reply of `norm` and `distance` is correctly rounded (`root_midpoint_nearest`, S28 of `VectorOps.v`).

The failure codes are `UnknownMethod`, `BadArgument`, `NotAVector`, `NotANumber`, `NonFiniteValue`, `MalformedNumber`, `DimensionMismatch` and `ValueOutOfRange`.
The process reserves its work on the host-work budget of the deploy, and a rejection fails the deploy (`rholang/src/rust/interpreter/reified/system_processes.rs:365-389`).

**Rule SP-3: `embed`.** The argument is one text, or a non-empty list of texts (`rholang/src/rust/interpreter/reified/system_processes.rs:415-458`).
A call holds at most 64 texts and at most 64 KiB of text. Otherwise the reply is `EmbeddingRateLimited`.

Play asks the embedder of the node, and it replies `[true, vector, model]` for one text or `[true, [vector…], model]` for a list.
Each vector is a list of `GFloat32` values. The failure codes are `BadArgument`, `EmbeddingRateLimited`, `EmbeddingUnavailable` and `NonFiniteEmbedding`.

**Rule SP-4: `ann`.** Play and replay both make the checks of the call against consensus state, in this order ([decision 67](decision-register.md#decision-67), `rholang/src/rust/interpreter/reified/system_processes.rs:541-586`):

1. `space` is a native VectorDB space (`NotAVectorDbSpace`) with a mirror (`MirrorDisabled`).
2. `query` is a text, or a numeric vector of the dimensions of the space whose elements are finite as `f32` values (`DimensionMismatch`, `BadArgument`).
3. `options` is a map of the one key `k`, an `Int` in $`[1, \mathit{capacity}]`$ (`InvalidOption`).

In play, the embedder of the node embeds a text query, and the mirror of the node answers the nearest records of `channel` (`rholang/src/rust/interpreter/reified/system_processes.rs:641-686`).
The reply is `[true, ids, distances, watermark]`.
It holds at most $`k`$ record ids as 32-byte byte arrays, their distances as `GFloat32` values, and the watermark as an `Int`.
A mirror that does not answer gives the reply `MirrorUnavailable`.
The ids are hints. Only the query param `ids=` of a receive turns them into a decision, and the exact order of rule VD-4 decides ([decision 58](decision-register.md#decision-58)).

**Rule SP-5: recorded replies.** `embed` and `ann` are recorded processes: they are in `non_deterministic_ops()`, so the dispatcher records each reply on the produce event of the reply.
A failure reply is recorded too.
Replay produces the recorded reply and asks no embedder and no mirror, so two nodes with other services compute the same state.
Both processes run in native execution ([decision 68](decision-register.md#decision-68)).

**Rule SP-6: the `config` reply of a mirrored space.** The reply of `space!?("config")` holds one more entry for a space with a mirror (`rholang/src/rust/interpreter/reified/handle.rs:745-785`).
The entry `"mirror"` is the map `{"backend": "chroma", "collection": c}`.
Here `c` is `rrs-` and the lowercase hex of the first 16 bytes of the space id.
The stored config record does not change, so the name of the collection is never consensus state.

## 10. The ChromaDB mirror

A VectorDB space with the factory param `mirror = "chroma"` has a mirror on each node ([decision 58](decision-register.md#decision-58)).
The mirror is a copy of the records of the space in the ChromaDB database of the node.
It is a hint index for `ann`: it never decides a COMM, and nothing in consensus reads it.
Section [N16 of the design](design.md#n16-phase-p6-the-chromadb-search-backend) explains the code, and [`docs/rholang/chromadb.md`](../../rholang/chromadb.md) explains how to run a node with a mirror.

**Rule MR-1: net writes.** After each commit of a transaction of a mirrored space, the node computes the net writes of the transaction (`rholang/src/rust/interpreter/spaces/mirror.rs:236-313`).
The first and the last event of each record id decide:

- A record that the transaction stores and keeps is an upsert, when the record has a vector whose elements are finite as `f32` values.
- A non-persistent record that the transaction removes is a delete.
- A record that the transaction stores and removes again, and a record that a peek removes and Rule E stores again, write nothing.

A delete names every removed record, also a record without a vector. The mirror never holds such a record, so the delete has no effect.
The computation has no meter, no error path and no wait, so it never changes the outcome, the events or the charges of a transaction.

**Rule MR-2: the journal.** The journal of an evaluation keeps the last net write of each record id of each collection (`rholang/src/rust/interpreter/spaces/mirror.rs:408-457`).
The start of each root evaluation drops a journal that no result settled (`rholang/src/rust/interpreter/reified/runtime.rs:102-118`).
So an exploratory evaluation never publishes, and its writes never reach the next deploy.

**Rule MR-3: publication (M1).** A final evaluation result with no errors publishes the journal as one batch for each collection, and every other result drops it (`rholang/src/rust/interpreter/reified/runtime.rs:315-320`, [decision 66](decision-register.md#decision-66)).
The two settle points are `rholang/src/rust/interpreter/interpreter.rs:257` for play and `rholang/src/rust/interpreter/rho_runtime.rs:1808` for native replay.
Native replay publishes to the mirror of the node that replays, so a validator keeps its mirror current from the blocks that it certifies.
A node that plays and then certifies one deploy publishes its batch twice, and the upserts by id make the second publication change nothing.

**Rule MR-4: the writer.** One task of the node applies the batches in publication order, from a bounded queue of 1,024 batches (`rholang/src/rust/interpreter/reified/vectordb_services.rs:455-499`).
For each batch, it makes the collection, registers the channels of the batch, upserts the records and deletes the ids.
A batch that fails is applied again from its start, with at most 3 attempts. The pauses between the attempts are 100 and 200 milliseconds.
A publication never waits: a batch that finds the queue full, or that fails after its attempts, marks its collection dirty.

**Rule MR-5: the reconciler.** On each finalized block, the reconciler of the node runs one pass for the newest finalized block that no pass took yet (`rholang/src/rust/interpreter/reified/mirror_reconciler.rs:139-212`).
The pass makes each dirty collection and registers its channels first.
It then repairs each collection whose watermark is below the block, or that is dirty.

A repair sets each registered channel to the records of the finalized state: it deletes the orphans and adds the missing records.
A repair that finishes sets the watermark to the larger of the block and the old watermark. A failed call keeps the watermark, and a dirty collection stays dirty.

**Rule MR-6: the limits of the mirror.** The mirror reflects the finalized view of the registered channels, and `ann` reports the watermark of that view.
The optimistic writes of later blocks leave the mirror at each pass, until those blocks are finalized.
After an LFS bootstrap, the records of a channel that the node never journaled are absent until the channel changes.
After a restart, the dirty marks of the writer are lost. A failed batch is repaired only when its channels were registered before the failure, or when they change again.

**Rule MR-7: nodes without a mirror.** A node without the `chromadb` feature, or without a ChromaDB endpoint, has no writer, and it starts no reconciler (`rholang/src/rust/interpreter/reified/mirror_reconciler.rs:373-411`).
Its publications drop their batches, and `ann` replies `MirrorUnavailable` in play.
Each node of a shard has its own database (`CHROMA_DATABASE`), so a node never writes the mirror of another node ([decision 61](decision-register.md#decision-61)).

## References

<a id="ref-bbg95"></a>[BBG95] H. Berenson, P. Bernstein, J. Gray, J. Melton, E. O'Neil, and P. O'Neil, "A critique of ANSI SQL isolation levels," in *Proceedings of the 1995 ACM SIGMOD International Conference on Management of Data*, ACM, 1995, pp. 1–10. [doi:10.1145/223784.223785](https://doi.org/10.1145/223784.223785)
