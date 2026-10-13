# 21. Reified Spaces

A Reified space is a tuple space that a program creates and uses as a value [[FIPS-RRS]](#ref-fips-rrs).
A factory creates the space, and `use` blocks and space annotations route the channels of a program into it.
The space decides how its sends and receives meet, for example first in, first out.
This chapter teaches the features of the language with examples.
The [Reified RSpaces specification](../rspace/reified-rspaces/specification.md) states the rules of every space family, and the [syntax contract](../rspace/reified-rspaces/syntax-contract.md) states the syntax.
The [agent API](../rspace/reified-rspaces/agent-api.md) states the calls, the replies and the errors.

Each code block of this chapter is the text of one file of `examples/reified_rspaces/`.
The test `rholang/tests/reified_examples_execute.rs` runs each file and checks its result, so each example works as the chapter shows it.
An example reports its result on a public channel, such as `@"rrs.example.queue"`, so the test can read it.

## Contents

- [Terms](#terms)
- [A first space](#a-first-space)
- [Space families](#space-families)
- [Creating a space](#creating-a-space)
- [Using a space](#using-a-space)
- [Inner collections](#inner-collections)
- [Outer stores](#outer-stores)
- [Qualifiers](#qualifiers)
- [The handle and its methods](#the-handle-and-its-methods)
- [getSpaceAgent and assertEqual](#getspaceagent-and-assertequal)
- [Theories](#theories)
- [Receive queries](#receive-queries)
- [Joins](#joins)
- [VectorDB spaces](#vectordb-spaces)
- [Determinism](#determinism)
- [When a deploy aborts](#when-a-deploy-aborts)
- [Writing your own space](#writing-your-own-space)
- [The FIPS example](#the-fips-example)
- [All examples](#all-examples)
- [References](#references)

## Terms

| Term | Meaning |
|---|---|
| [Default space](../rspace/reified-rspaces/glossary.md#rspace) | The tuple space of the node, which every program uses when it uses no Reified syntax. |
| [Space](../rspace/reified-rspaces/glossary.md#space) | A tuple space with its own channels. A [native space](../rspace/reified-rspaces/glossary.md#native-space) is a space that the node implements. |
| [Space family](../rspace/reified-rspaces/glossary.md#space-family) | One kind of native space: an outer store paired with an inner collection, such as `hashmap:queue`. |
| [Outer store](../rspace/reified-rspaces/glossary.md#outer-store) | The part of a family that maps each channel of the space to a collection. |
| [Collection](../rspace/reified-rspaces/glossary.md#collection) | The records and the waiting receives of one channel of a space. |
| [Inner collection](../rspace/reified-rspaces/glossary.md#inner-collection) | The part of a family that decides how the records and the receives of one collection meet. |
| [Qualifier](../rspace/reified-rspaces/glossary.md#qualifier) | The lifetime and the concurrency of a space: `"default"`, `"temp"` or `"seq"`. |
| [Factory](../rspace/reified-rspaces/glossary.md#factory) | The system process that creates the spaces of one family. A URN such as `rho:space:hashmap:queue` names it. |
| [Handle](../rspace/reified-rspaces/glossary.md#handle) | The name that a factory gives for a new space. A program calls the methods of the space on it. |
| [Space name](../rspace/reified-rspaces/glossary.md#space-name) | A name that a space makes. A send or a receive on it reaches the space in every context. |
| [Use context](../rspace/reified-rspaces/glossary.md#use-context) | The space of the innermost `use` block around a process. |
| [Theory](../rspace/reified-rspaces/glossary.md#theory) | A rule that decides which values a space accepts. |

## A first space

```rholang
// A queue space gives back its records in the order in which they arrived:
// first in, first out.
//
// Each send waits for the reply of the handle method produce, so the arrival
// order is "A", "B", "C". The three receives take the records in that order.
// The result leaves the space on the name `out`, which this deploy creates
// outside the use block, so it routes to the default space.
new factory(`rho:space:hashmap:queue`), out in {
  for (queue <- factory!?("default")) {
    queue!?("produce", "jobs", ["A"]);
    queue!?("produce", "jobs", ["B"]);
    queue!?("produce", "jobs", ["C"]);
    use queue {
      for (@first <- @"jobs") {
        for (@second <- @"jobs") {
          for (@third <- @"jobs") {
            out!([first, second, third])
          }
        }
      }
    }
  } |
  for (@taken <- out) { @"rrs.example.queue"!(taken) }
}
```

The URN `rho:space:hashmap:queue` names the factory of the family `hashmap:queue`.
The call `factory!?("default")` creates a space with the qualifier `"default"`, and the receive binds its handle to `queue`.
The call `queue!?("produce", "jobs", ["A"])` stores the record `"A"` on the channel `"jobs"` of the space, and the `;` waits for the reply.
So the records arrive in the order `"A"`, `"B"`, `"C"`.

In the block `use queue { … }`, the forged name `@"jobs"` routes to the collection of `"jobs"` in the space.
A queue gives back its records in the order of their arrival, so the three receives take `"A"`, `"B"` and `"C"`.
The name `out` comes from a `new` outside the `use` block, so it stays in the default space.
The deploy reports `["A", "B", "C"]`.

## Space families

A family pairs one outer store with one inner collection, and its URN is `rho:space:` followed by the two names.
Table 1 lists the 32 families.

*Table 1. The space families.*

| Outer store | `bag` | `queue` | `stack` | `set` | `cell` | `priorityqueue` | `vectordb` |
|---|---|---|---|---|---|---|---|
| `hashmap` | Yes | Yes | Yes | Yes | Yes | Yes | Yes |
| `pathmap` | Yes | Yes | Yes | Yes | Yes | Yes | No |
| `array` | Yes | Yes | Yes | Yes | Yes | Yes | No |
| `vector` | Yes | Yes | Yes | Yes | Yes | Yes | Yes |
| `hashset` | Yes | Yes | Yes | Yes | Yes | Yes | No |

For example, `rho:space:pathmap:stack` names the family of PathMap stores with stack collections.
Two URNs of the FIPS name families too: `rho:space:HashMapBagSpace` names `hashmap:bag`, and `rho:space:PathMapSpace` names `pathmap:bag`.

## Creating a space

A factory call has three optional parts, in this order:

```text
factory!?(qualifier, free Theory(); name=value, …)
```

- The qualifier is a string: `"default"`, `"temp"` or `"seq"`. Without it, the space gets `"default"`.
- The theory is one of the builtin theories `Nat`, `Int`, `String`, `Bool` and `Any`, without arguments ([Theories](#theories)).
- The params come after the `;`, and each one has a name. Table 2 gives the params of each family.

*Table 2. The factory params.*

| Families | Params |
|---|---|
| Every `array` family | `size` (required): the number of names of the space, an `Int` of at least 1. `cyclic` (optional): `true` gives each name again after `size` names. |
| Every `priorityqueue` family | `priorities` (optional): a bound on the levels, an `Int` of at least 1 |
| `hashmap:vectordb`, `vector:vectordb` | `capacity` and `dimensions` (required), `metric` and `mirror` (optional) ([VectorDB spaces](#vectordb-spaces)) |
| Every other family | None |

The factory replies the handle of the new space.
A call with a wrong part aborts the deploy before the space exists.
Table 3 gives the codes, and each example shows one of them.

*Table 3. The errors of a factory call.*

| Code | Cause | Example |
|---|---|---|
| `BadConstructorArgument` | An argument that is not a qualifier, a theory or a params section. A HashSet space with another qualifier than `"seq"` gives it too. | `factory-rejects-a-bad-argument` |
| `UnknownTheory` | A theory that is not a builtin theory | `factory-rejects-an-unknown-theory` |
| `PositionalParamsUnsupported` | A param without a name | `factory-rejects-a-positional-param` |
| `UnknownParam` | A param that the family does not take | `factory-rejects-an-unknown-param` |
| `MissingParam` | A required param that is absent | `factory-rejects-a-missing-param` |
| `InvalidParam` | A param with a value outside its range | `factory-rejects-an-invalid-param` |
| `PersistentFactoryCall` | A persistent call `!!`, which the runtime would repeat forever | `factory-rejects-a-persistent-call` |

For example, a call to the factory of a queue with the param `capacity` aborts with `UnknownParam`, because a queue takes no params:

```rholang
// The params of a factory follow the schema of its family. A queue has no
// params, so a named param aborts the deploy with UnknownParam. The message of
// the error names the param.
new factory(`rho:space:hashmap:queue`) in {
  for (queue <- factory!?("default"; capacity=4)) { Nil }
}
```

## Using a space

### `use` blocks

The block `use s { P }` runs `P` in the context of the space `s`.
Inside the block, a forged name such as `@"inbox"` routes to the collection of that name in `s`.
A `new` inside the block makes its names in `s`.
A name that comes from outside the block keeps its own space, and a system channel always stays in the default space.

```rholang
// Two spaces keep their channels apart. A forged name such as @"inbox" inside
// a use block routes to the space of that block, so @"inbox" in one space and
// @"inbox" in the other are two channels. A name that `new` creates outside
// every use block stays in the default space.
new factory(`rho:space:hashmap:bag`), out in {
  for (left <- factory!?("default")) {
    for (right <- factory!?("default")) {
      use left { @"inbox"!("left mail") } |
      use right { @"inbox"!("right mail") } |
      use left { for (@mail <- @"inbox") { out!(["left", mail]) } } |
      use right { for (@mail <- @"inbox") { out!(["right", mail]) } }
    }
  } |
  for (@delivery <= out) { @"rrs.example.isolation"!(delivery) }
}
```

The two spaces both have a channel `@"inbox"`, but each space has its own collection for it.
So each receive takes the mail of its own space.
Two `use` blocks of one space are one context, so the sends and the receives of `left` meet across the blocks.

### Space annotations

The binder `x : s` of a `new` makes the name `x` in the space `s`, also outside every `use` block.
A space name routes to its own space in every context.

```rholang
// `new x : s` creates the name x in the space s, wherever the `new` stands. A
// name of a space routes to that space by its id, also outside every use
// block. A join of two names of one space is allowed.
new factory(`rho:space:hashmap:queue`), out in {
  for (queue <- factory!?("default")) {
    new first : queue, second : queue in {
      first!("one") | second!("two") |
      for (@a <- first & @b <- second) { out!([a, b]) }
    }
  } |
  for (@pair <- out) { @"rrs.example.annotations"!(pair) }
}
```

The two names belong to the queue space, so their sends and the join reach that space.
A join can take its channels from one space, as the [joins](#joins) section explains.

### How the node routes a channel

Table 4 states where a channel goes, and Figure 1 shows the full decision of the reducer.

*Table 4. The routing of a channel.*

| Channel | Where it goes |
|---|---|
| A name that a space made, by `new` in its context, by an annotation, or by `gensym` | Its own space, in every context |
| A forged name such as `@"jobs"` inside `use s` | The space `s` |
| A forged name outside every `use` block | The default space |
| A name that a `new` made outside every `use` block | The default space |
| A system channel, such as a factory or `rho:io:stdout` | The default space |

![Routing decision as an activity. The reducer removes the bundle of the channel and classifies it. A sole GPrivate with an id of 1 or 2 bytes is System. A decoded id of KIND C, H, X or B is DefaultObject. A decoded id of KIND D, I, A or S is Storage. A decoded id of KIND N is SpaceName with its native space. Any other sole unforgeable is Unforgeable. A tuple whose head is AGENT_TAG is AgentTagged. Every other channel is Forged. Rule R1 sends System, DefaultObject and Unforgeable to the default space, R2 sends Storage to the default space, R3 sends a space name to the collection key(c) of its own space in every context, R4 sends an agent-tagged name to the user agent: since P8 step 2 a send there becomes the call on the agent. Since P8 step 3 a receive whose binds all route to the agent, with no query, becomes T_recv: one persistent matcher receive for each bind, the body receive, and last the call A'!(sink, "consume", pairs, arrow, bundle+{b}) on the agent. A mix of targets fails with CrossSpaceJoin, and a query fails with QuerySpecUnsupportedOnAgentSpace. R5 sends a forged channel to the collection key(c) of the use context, or to the default space without one. The default route makes the baseline call with identical arguments, and a space route builds the identity on D(sid, k) (R6) and runs one space intent. The R7 backend routes the replies of system processes with the same rules and no use context, and the bundle with both flags of an agent-minted name reaches its agent (R7A).](../rspace/reified-rspaces/diagrams/12-routing-decision.svg)

*Figure 1. Source: [diagrams/12-routing-decision.puml](../rspace/reified-rspaces/diagrams/12-routing-decision.puml), rendered by [diagrams/render.sh](../rspace/reified-rspaces/diagrams/render.sh).*

## Inner collections

The inner collection of a family decides which record a receive takes and which receive a send meets.
Table 5 compares them.
The last column says what happens when two parallel blocks both use one collection.
"Merge" means that the blocks merge by the rules of ordinary channels, so they conflict only when both take one record.
"Conflict" means that every two such blocks conflict, and the merge keeps the changes to the collection from only one block.

*Table 5. The inner collections.*

| Inner collection | A receive takes | Equal records | Two parallel blocks |
|---|---|---|---|
| `bag` | The first matching record in a fixed canonical order | The collection keeps each one | Merge |
| `set` | The first matching value, with every stored copy of it | The receives see one value for each class of equal records | Merge |
| `queue` | Only the oldest record, the head | The collection keeps each one | Conflict |
| `stack` | Only the newest record, the head | The collection keeps each one | Conflict |
| `cell` | The one record | A cell holds at most one record and one waiting receive | Conflict |
| `priorityqueue` | The head of the highest level that the receive may see | The collection keeps each one | Conflict |
| `vectordb` | The most similar record, or a list of records | The collection keeps each one, up to its capacity | Merge |

On a queue, a stack, a cell and a priority queue, a head that does not match blocks the records behind it.
A receive with a `where` guard checks the guard once, on the record that it would take, and a refusal makes it wait.
The [specification](../rspace/reified-rspaces/specification.md#3-inner-collections) states the rules of peeks and persistent records for each collection.

### Stack

```rholang
// A stack space gives back its records in the reverse order of their arrival:
// last in, first out.
//
// Each send waits for the reply of the handle method produce, so the arrival
// order is "A", "B", "C". The three receives take "C", "B" and "A".
new factory(`rho:space:hashmap:stack`), out in {
  for (stack <- factory!?("default")) {
    stack!?("produce", "plates", ["A"]);
    stack!?("produce", "plates", ["B"]);
    stack!?("produce", "plates", ["C"]);
    use stack {
      for (@first <- @"plates") {
        for (@second <- @"plates") {
          for (@third <- @"plates") {
            out!([first, second, third])
          }
        }
      }
    }
  } |
  for (@taken <- out) { @"rrs.example.stack"!(taken) }
}
```

### Bag

```rholang
// A bag space is the default discipline of an RSpace: a receive takes any
// matching record, and the records have no order.
//
// The deploy sends 1, 2 and 3 in parallel. A receive with the pattern 2 takes
// the record 2, and two receives take the other two records.
new factory(`rho:space:hashmap:bag`), out in {
  for (bag <- factory!?("default")) {
    use bag {
      @"numbers"!(1) | @"numbers"!(2) | @"numbers"!(3) |
      for (@2 <- @"numbers") {
        out!(["matched", 2]) |
        for (@x <- @"numbers") {
          for (@y <- @"numbers") { out!(["rest", x + y]) }
        }
      }
    }
  } |
  for (@value <= out) { @"rrs.example.bag"!(value) }
}
```

The receive with the pattern `2` takes only the record 2.
The other two receives take the records 1 and 3 in either order, and their sum is 4.

### Set

```rholang
// A set space holds each value once. A receive takes the value, and it also
// retires every equal record that arrived earlier.
//
// The deploy sends "a" twice. The first receive takes "a". Then the deploy
// sends "probe", and the second receive takes "probe": the second "a" left
// the set with the first one. In a bag space the second receive could take
// the second "a".
new factory(`rho:space:hashmap:set`), out in {
  for (set <- factory!?("default")) {
    set!?("produce", "tags", ["a"]);
    set!?("produce", "tags", ["a"]);
    use set {
      for (@first <- @"tags") {
        out!(first) |
        set!?("produce", "tags", ["probe"]);
        for (@second <- @"tags") { out!(second) }
      }
    }
  } |
  for (@value <= out) { @"rrs.example.set"!(value) }
}
```

### Cell

```rholang
// A cell space holds at most one record on each channel. A receive empties
// the cell, and then the cell takes a new record.
//
// The deploy stores 1, takes it, stores 2 and takes it.
new factory(`rho:space:hashmap:cell`), out in {
  for (cell <- factory!?("default")) {
    cell!?("produce", "counter", [1]);
    use cell {
      for (@first <- @"counter") {
        out!(first) |
        cell!?("produce", "counter", [first + 1]);
        for (@second <- @"counter") { out!(second) }
      }
    }
  } |
  for (@value <= out) { @"rrs.example.cell"!(value) }
}
```

A second send to a full cell aborts the deploy with `CellOccupied`:

```rholang
// A cell holds at most one record. A second send to a full cell aborts the
// deploy with CellOccupied.
new factory(`rho:space:hashmap:cell`) in {
  for (cell <- factory!?("default")) {
    cell!?("produce", "counter", [1]);
    cell!?("produce", "counter", [2]).
  }
}
```

### Priority queue

```rholang
// A priority queue keeps one queue for each level. Level 0 is the highest,
// and a send without a priority goes to the lowest level.
//
// The deploy sends "low" with no priority, "two" at level 2 and "zero" at
// level 0. A receive without a level takes the head of the highest level
// that holds a record, so the receives take "zero", "two" and "low". A
// receive with `priority=2` only takes from level 2.
new factory(`rho:space:hashmap:priorityqueue`), out in {
  for (queue <- factory!?("default")) {
    queue!?("produce", "tasks", ["low"]);
    queue!?("produce", "tasks", ["two"]; priority=2);
    queue!?("produce", "tasks", ["zero"]; priority=0);
    use queue {
      for (@first <- @"tasks") {
        for (@second <- @"tasks") {
          for (@third <- @"tasks") {
            out!([first, second, third]) |
            @"lanes"!("lane two"; priority=2) |
            @"lanes"!("lane zero"; priority=0) |
            for (@lane <- @"lanes" ~ (; priority=2)) { out!(lane) }
          }
        }
      }
    }
  } |
  for (@value <= out) { @"rrs.example.pq"!(value) }
}
```

A send takes the level of its param `priority`, and a send without it goes to the lowest level.
A receive with the query `~ (; priority=p)` sees only the level `p`.
A receive without a query sees every level, the highest first.

## Outer stores

The outer store of a family decides which channels a space has and how it maps each channel to a collection.

### HashMap

A HashMap space takes every channel, and two equal channels name one collection.
Most examples of this chapter use HashMap spaces.

### PathMap

A PathMap space treats a list channel as a path, for example `@["sensors", "north"]`.
A send on a path is also a send on each prefix of the path.
A receive on a prefix gets each value re-rooted under the rest of its path.
The [PathMap delivery](../rspace/reified-rspaces/pathmap-prefix-delivery.md) explains the model.

```rholang
// The PathMap example of the Reifying RSpaces FIPS. In a PathMap space a send
// on a path is also a send on each prefix of the path. A receive on the
// prefix [0, 1] takes a record of a longer path and gets it re-rooted under
// the suffix of the path:
//
//   @[0, 1, 2]!({|"hi"|})   binds {|[2, "hi"]|}
//   @[0, 1, 2]!(42)         binds [2, 42]
//
// The deploy sends three path maps and the number 42, and four receives on
// the prefix [0, 1] take them.
new factory(`rho:space:PathMapSpace`), out in {
  for (space <- factory!?("default")) {
    use space {
      @[0, 1, 2]!({|"hi"|}) | @[0, 1, 2]!({|"hello"|}) | @[0, 1, 3]!({|"there"|}) |
      @[0, 1, 2]!(42) |
      for (@a <- @[0, 1]) { out!(a) } |
      for (@b <- @[0, 1]) { out!(b) } |
      for (@c <- @[0, 1]) { out!(c) } |
      for (@d <- @[0, 1]) { out!(d) }
    }
  } |
  for (@value <= out) { @"rrs.example.pathmap"!(value) }
}
```

The receives take the four values in any order, and the deploy reports `[2, 42]`, `{|[2, "hi"]|}`, `{|[2, "hello"]|}` and `{|[3, "there"]|}`.
Figure 2 shows how the node re-roots one value.

![Activity diagram of R(s, v), the delivery of one argument v of a datum on the path p to a receive on the prefix q. The suffix s is p without q. When s is empty, v is delivered as it is. When v is one EPathMap, the adapter decodes it with its paths in key order and one path for each key. An empty map delivers the unit {{| s |}}, and every other map delivers {{| s ++ elems(e) |}} for each entry e, and the adapter encodes the map in the canonical entry form. Every other value is delivered as the list [s_1, ..., s_k, v].](../rspace/reified-rspaces/diagrams/20-pathmap-reroot.svg)

*Figure 2. Source: [diagrams/20-pathmap-reroot.puml](../rspace/reified-rspaces/diagrams/20-pathmap-reroot.puml), rendered by [diagrams/render.sh](../rspace/reified-rspaces/diagrams/render.sh).*

```rholang
// A PathMap space as a tree of channels. A receive on a prefix sees the
// records of every path below it, and a receive on a full path sees only the
// records of that path.
//
// The deploy stores two readings under ["sensors", "north"] and one under
// ["sensors", "south"]. A receive on ["sensors", "south"] takes its reading
// with an empty suffix. Then two receives on ["sensors"] take the two north
// readings, re-rooted under "north".
new factory(`rho:space:pathmap:bag`), out in {
  for (tree <- factory!?("default")) {
    tree!?("produce", ["sensors", "north"], [10]);
    tree!?("produce", ["sensors", "north"], [11]);
    tree!?("produce", ["sensors", "south"], [20]);
    use tree {
      for (@south <- @["sensors", "south"]) {
        out!(["south", south]) |
        for (@first <- @["sensors"]) {
          for (@second <- @["sensors"]) { out!(["prefix", first]) | out!(["prefix", second]) }
        }
      }
    }
  } |
  for (@value <= out) { @"rrs.example.pathmap.tree"!(value) }
}
```

### Array and Vector

An Array space has `size` names, and a Vector space has no fixed number of names.
Each `new` in the context of the space takes the next free index, and so does the method `gensym`.
A channel that is not an index name of the space aborts with `NotAnIndexName`, so a forged name never reaches the space.
A fixed Array space that has given all its names aborts the next `new` with `OutOfNames`.

```rholang
// An Array space has a fixed number of names. Each `new` inside a use block
// of the space takes the next free index, and the names route to the space
// by their index.
//
// The space has size 2. The deploy creates two names, stores a value on each,
// and takes both values.
new factory(`rho:space:array:bag`), out in {
  for (array <- factory!?("default"; size=2)) {
    use array {
      new first, second in {
        first!("x") | second!("y") |
        for (@a <- first & @b <- second) { out!([a, b]) }
      }
    }
  } |
  for (@pair <- out) { @"rrs.example.array"!(pair) }
}
```

```rholang
// A Vector space grows: each `new` inside a use block takes the next index,
// with no fixed size. The handle method gensym takes the next index too.
//
// The deploy creates three names with `new`, one with gensym, and sends on
// each. The receive joins the four names.
new factory(`rho:space:vector:bag`), out in {
  for (vector <- factory!?("default")) {
    for (fourth <- vector!?("gensym")) {
      use vector {
        new first, second, third in {
          first!(1) | second!(2) | third!(3) | fourth!(4) |
          for (@a <- first & @b <- second & @c <- third & @d <- fourth) {
            out!(a + b + c + d)
          }
        }
      }
    }
  } |
  for (@sum <- out) { @"rrs.example.vector"!(sum) }
}
```

### HashSet

A HashSet space holds units: a send carries no data, and a receive binds nothing.
A send with data aborts with `PayloadNotEmpty`, and a receive with a pattern aborts with `PatternNotEmpty`.
A HashSet space takes only the qualifier `"seq"`.

```rholang
// A HashSet space holds units: a send carries no data, and a receive binds
// nothing. HashSet spaces take only the `seq` qualifier. With the set
// collection a channel holds one unit, and with the bag collection it counts
// its units.
//
// The deploy sends two units to "seen" in a set space and in a bag space. In
// each space a receive takes one unit, reports it, and then waits for a second
// unit. The bag holds the second unit, and the set does not. In a seq space
// each later use of a channel must follow the earlier use, so the second
// receive stands in the body of the first.
new setFactory(`rho:space:hashset:set`), bagFactory(`rho:space:hashset:bag`), out in {
  for (set <- setFactory!?("seq")) {
    for (bag <- bagFactory!?("seq")) {
      set!?("produce", "seen", []);
      set!?("produce", "seen", []);
      bag!?("produce", "seen", []);
      bag!?("produce", "seen", []);
      {
        use set {
          for ( <- @"seen") {
            out!("set took a unit") |
            for ( <- @"seen") { out!("set took a second unit") }
          }
        } |
        use bag {
          for ( <- @"seen") {
            out!("bag took a unit") |
            for ( <- @"seen") { out!("bag took a second unit") }
          }
        } |
        for (@qualifier <- set!?("qualifier")) { out!(["set", qualifier]) }
      }
    }
  } |
  for (@value <= out) { @"rrs.example.hashset"!(value) }
}
```

## Qualifiers

The qualifier of a space sets its lifetime and its concurrency (Table 6).

*Table 6. The qualifiers.*

| Qualifier | Lifetime | Use of one channel | A name of the space as data |
|---|---|---|---|
| `"default"` | The records stay after the deploy, and later deploys can use the space | Many processes, and many deploys | Allowed |
| `"temp"` | One deploy: the node removes the records at the end of the deploy | Many processes of the deploy | Allowed |
| `"seq"` | One deploy, as for `"temp"` | One chain of uses: each use follows the previous use | A send that carries the name aborts with `SeqSent` |

A `"seq"` space works only in native execution ([decision 73](../rspace/reified-rspaces/decision-register.md#decision-73)).

### Default

The first deploy stores a map and publishes the handle of its space:

```rholang
// The first deploy of the default example. A `default` space persists: its
// records enter the history like ordinary data, and its handle stays valid in
// later deploys. The deploy makes a default space, stores a map in it, and
// publishes the handle of the space on a public channel.
new factory(`rho:space:hashmap:bag`) in {
  for (space <- factory!?("default")) {
    use space { @"settings"!({"theme": "dark", "lang": "en"}) } |
    @"rrs.example.default.handle"!(*space)
  }
}
```

A later deploy takes the handle and reads the map:

```rholang
// The second deploy of the default example. It takes the handle that the
// first deploy published, asks the space for its qualifier, and takes the map
// that the first deploy stored. The example temp_space_ends_with_its_deploy
// shows the same steps on a temp space, which ends with its deploy.
new out in {
  for (space <- @"rrs.example.default.handle") {
    for (@qualifier <- space!qualifier()) {
      use space {
        for (@settings <- @"settings") { out!([qualifier, settings]) }
      }
    }
  } |
  for (@answer <- out) { @"rrs.example.qualifier.default"!(answer) }
}
```

### Temp

```rholang
// A `temp` space lives for one deploy: its records never enter the history,
// and the sweep at the end of the deploy removes them. Within its deploy a
// temp space works like any other space. The deploy stores a token in a temp
// space, takes it back with a receive in the space, and reports the qualifier.
// The example temp_space_ends_with_its_deploy shows the end of a temp space.
new factory(`rho:space:hashmap:bag`), out in {
  for (space <- factory!?("temp")) {
    for (@qualifier <- space!qualifier()) {
      use space {
        new token in {
          token!("session-token") |
          for (@value <- token) { out!([qualifier, value]) }
        }
      }
    }
  } |
  for (@answer <- out) { @"rrs.example.qualifier.temp"!(answer) }
}
```

A temp space ends with its deploy.
The first deploy of this example publishes the handle of a temp space:

```rholang
// The first deploy of the temp example. It makes a temp space, stores a value
// in it, and publishes the handle of the space on a public channel. A temp
// space lives for one deploy, so the sweep at the end of this deploy removes
// it.
new factory(`rho:space:hashmap:bag`) in {
  for (scratch <- factory!?("temp")) {
    scratch!?("produce", "notes", ["draft"]);
    @"rrs.example.temp.handle"!(*scratch)
  }
}
```

The second deploy calls the handle, and the call aborts with `DanglingHandle`:

```rholang
// The second deploy of the temp example. It takes the handle that the first
// deploy published and calls it. The temp space ended with the first deploy,
// so the call aborts the deploy with DanglingHandle.
for (scratch <- @"rrs.example.temp.handle") {
  for (@qualifier <- scratch!?("qualifier")) { Nil }
}
```

### Seq

```rholang
// A `seq` space lives for one deploy, like a temp space, and it adds two
// rules: each use of a channel must follow the previous use of that channel,
// and a name of the space never travels as data. The deploy stores a value on
// a name of a seq space with the produce method, and after the reply it takes
// the value with a receive, so the second use follows the first. The examples
// space_qualifiers_seq_sent and seq_rejects_concurrent_uses show the two rules.
new factory(`rho:space:hashmap:bag`), out in {
  for (space <- factory!?("seq")) {
    for (@qualifier <- space!qualifier()) {
      for (name <- space!gensym()) {
        for (_ <- space!produce(*name, ["local data"])) {
          for (@value <- name) { out!([qualifier, value]) }
        }
      }
    }
  } |
  for (@answer <- out) { @"rrs.example.qualifier.seq"!(answer) }
}
```

A send that carries a name of a seq space aborts with `SeqSent`:

```rholang
// A name of a `seq` space must never travel as data. A send that carries such
// a name aborts the deploy with SeqSent, before anything is stored.
new factory(`rho:space:hashmap:bag`) in {
  for (space <- factory!?("seq")) {
    for (name <- space!?("gensym")) {
      @"leak"!(*name)
    }
  }
}
```

Two uses of one channel in parallel abort with `SeqConcurrent`:

```rholang
// In a seq space each use of a channel must follow the previous use of that
// channel. A send and a receive that run in parallel on one channel abort the
// deploy with SeqConcurrent.
new factory(`rho:space:hashmap:queue`) in {
  for (space <- factory!?("seq")) {
    for (x <- space!gensym()) {
      x!(1) | for (@value <- x) { Nil }
    }
  }
}
```

## The handle and its methods

A space is an agent of the Agents FIPS [[FIPS-AGENTS]](#ref-fips-agents), and its handle answers five methods (Table 7).
The call `space!m(args)` is the method-call sugar of that FIPS: it sends the call with a fresh reply name.
So `for (name <- space!gensym())` binds the reply of `gensym`, and `space!produce(ch, msg).` waits for the reply of `produce`.

*Table 7. The methods of a handle.*

| Method | Arguments | Reply |
|---|---|---|
| `gensym` | None | A new name of the space |
| `produce` | A channel and the list of the data. The param `persistent=true` stores a persistent record, and the send params of the collection, such as `priority`, also apply. | `Nil`, after the send |
| `consume` | The matchers, the arrow and the body of a receive that the node translated | `Nil`, after the receive |
| `qualifier` | None | `"default"`, `"temp"` or `"seq"` |
| `config` | None | The map of the space: `spaceId`, `outer`, `inner`, `qualifier`, `params` and `theory` |

```rholang
// A space is an agent of the Agents FIPS. Its handle answers the methods
// gensym, produce, consume, qualifier and config. The method-call sugar
// `space!m(args)` sends the call with a reply channel.
//
// The deploy asks for a fresh name, stores a value on it with produce, reads
// the qualifier and the config, and takes the value with a receive in the
// space. The config is a map, and the deploy reads its outer store and its
// inner collection. The example agent_wrapper_forwards_to_native shows
// consume, which takes the matchers that the node builds for a receive.
new factory(`rho:space:hashmap:bag`), out in {
  for (space <- factory!?("default")) {
    for (name <- space!gensym()) {
      for (_ <- space!produce(*name, ["stored by produce"])) {
        for (@qualifier <- space!qualifier()) {
          for (@config <- space!config()) {
            for (@value <- name) {
              out!([qualifier, config.get("outer"), config.get("inner"), value])
            }
          }
        }
      }
    }
  } |
  for (@answer <- out) { @"rrs.example.handle"!(answer) }
}
```

A program cannot build the matchers of `consume` itself.
A user agent gets them from the node, and the [own-space section](#writing-your-own-space) shows a call of `consume`.

## getSpaceAgent and assertEqual

The system process `rho:space:getSpaceAgent` replies the factory of the family of a space.
The system process `rho:assert:assertEqual` replies `true` when its two values are equal after sorting, and `false` otherwise.

```rholang
// getSpaceAgent gives the factory capability of a native space, and
// assertEqual compares two values by their sorted pars. The FIPS alias
// rho:space:HashMapBagSpace and the URN rho:space:hashmap:bag name one factory.
new HMB(`rho:space:HashMapBagSpace`), bag(`rho:space:hashmap:bag`),
    getSpaceAgent(`rho:space:getSpaceAgent`), assertEqual(`rho:assert:assertEqual`), out in {
  for (space <- HMB!?("default")) {
    for (@capability <- getSpaceAgent!?(*space)) {
      for (@same <- assertEqual!?(capability, *HMB)) {
        for (@alias <- assertEqual!?(*HMB, *bag)) { out!([same, alias]) }
      }
    }
  } |
  for (@answers <- out) { @"rrs.example.agent"!(answers) }
}
```

## Theories

A factory call can name a builtin theory with `free Th()`.
The theory decides which values a send may add to the space (Table 8).
A send of a value outside the theory aborts the deploy with `TheoryRefuted`, before the space stores anything.

*Table 8. The builtin theories.*

| Theory | The values that it admits |
|---|---|
| `Nat` | One integer that is 0 or more |
| `Int` | One integer |
| `String` | One string |
| `Bool` | One Boolean |
| `Any` | Every value |

```rholang
// A theory restricts the data of a space. The builtin theory Nat admits only
// natural numbers.
//
// The deploy makes a bag space with `free Nat()` and sends 7 and 42. Both
// sends are admitted, and two receives take both.
new factory(`rho:space:HashMapBagSpace`), out in {
  for (space <- factory!?(free Nat())) {
    use space {
      @"numbers"!(7) | @"numbers"!(42) |
      for (@a <- @"numbers") { for (@b <- @"numbers") { out!(a + b) } }
    }
  } |
  for (@sum <- out) { @"rrs.example.theory"!(sum) }
}
```

```rholang
// The builtin theory Nat refuses a value that is not a natural number. A send
// of -1 to a space with `free Nat()` aborts the deploy with TheoryRefuted.
new factory(`rho:space:HashMapBagSpace`) in {
  for (space <- factory!?(free Nat())) {
    use space {
      @"numbers"!(-1)
    }
  }
}
```

A value must be exactly one integer, so the string `"Nat(42)"` and the process `1 | 2` are not in `Nat`.
The [theory seam](../rspace/reified-rspaces/theory-seam.md) explains the design, and installed theories come with F1r3lang.

## Receive queries

A receive can give its collection a query with `~`:

```text
for (pattern <- channel ~ value) { … }
for (pattern <- channel ~ (payload…; name=value, …)) { … }
```

Only the priority queue and the VectorDB collections take queries.
A priority queue takes the query `~ (; priority=p)`, and a VectorDB collection takes a query vector and the keys of the [VectorDB section](#vectordb-spaces).
Every other collection, the default space and a user agent abort a query.

```rholang
// Only a VectorDB collection answers a query. A receive with a query on a bag
// aborts the deploy with QuerySpecUnsupported.
new factory(`rho:space:hashmap:bag`) in {
  for (bag <- factory!?("default")) {
    use bag {
      for (@number <- @"numbers" ~ ([1, 0]; k=1)) { Nil }
    }
  }
}
```

## Joins

A join receives on two or more channels at once, for example `for (@a <- x & @b <- y)`.
All the channels of a join must route to one space.
A join of one space works with every inner collection.
On a head-only collection, such as a queue, a join fires only as the head waiter of each of its collections.
A join across two spaces, or across a space and the default space, aborts with `CrossSpaceJoin`:

```rholang
// A join may take its channels from one space only. A receive that joins a
// channel of one space with a channel of another space aborts the deploy with
// CrossSpaceJoin.
new factory(`rho:space:hashmap:bag`) in {
  for (left <- factory!?("default")) {
    for (right <- factory!?("default")) {
      new a : left, b : right in {
        a!(1) | b!(2) |
        for (@x <- a & @y <- b) { Nil }
      }
    }
  }
}
```

## VectorDB spaces

A VectorDB space stores records that can carry a vector, and a receive takes the records that are most similar to a query vector.
The families are `rho:space:hashmap:vectordb` and `rho:space:vector:vectordb`.
Every decision of the space compares exact numbers, so every node selects the same records ([decision 54](../rspace/reified-rspaces/decision-register.md#decision-54)).

### Creating a VectorDB space

The factory takes the params `capacity` and `dimensions`, and it can take `metric` and `mirror`.

| Param | Rule |
|---|---|
| `capacity` | Required. The most records of each collection, an `Int` from 1 to 1,000,000. |
| `dimensions` | Required. The length of every vector of the space, an `Int` from 1 to 4,096. |
| `metric` | Optional. `cosine` (the default), `dot`, `euclidean`, `manhattan`, `hamming` or `jaccard`. |
| `mirror` | Optional. `"chroma"` keeps a copy of the records in ChromaDB for advisory queries ([ChromaDB](chromadb.md#reified-chromadb-mirror)). |

### Storing records and querying them

A send stores its payload as a record.
When the payload is one list of numbers, that list is the vector of the record.
A receive with a query vector takes the most similar record.

```rholang
// A VectorDB space stores records that carry a vector, and a receive with a
// query vector takes the most similar record. A query can name its own metric
// and threshold, and a query with `k` binds a list of the first k records, the
// most similar first. Every decision compares exact numbers, so every node
// selects the same records.
//
// The deploy stores three persistent records, so every query sees all of them.
// Then four queries run:
//   - with the defaults of the space: the metric cosine and no threshold;
//   - with the metric dot, which prefers the longest vector in the direction
//     of the query;
//   - with the threshold 0.9, which keeps only the records whose cosine with
//     the query is at least 0.9;
//   - with the metric, a threshold and k=2, which binds the list of the two
//     best records.
new vdb(`rho:space:hashmap:vectordb`), out in {
  for (space <- vdb!?("default"; capacity=8, dimensions=2)) {
    space!?("produce", "docs", [[1, 0]]; persistent=true);
    space!?("produce", "docs", [[3, 4]]; persistent=true);
    space!?("produce", "docs", [[0, 1]]; persistent=true);
    use space {
      for (@v <- @"docs" ~ ([1, 0])) { out!(["defaults", v]) } |
      for (@v <- @"docs" ~ ([1, 1]; metric="dot")) { out!(["dot", v]) } |
      for (@v <- @"docs" ~ ([0, 1]; threshold=0.9)) { out!(["threshold", v]) } |
      for (@v <- @"docs" ~ ([1, 0]; metric="cosine", threshold=0.5, k=2)) { out!(["top two", v]) }
    }
  } |
  for (@answer <= out) { @"rrs.example.vectordb"!(answer) }
}
```

The sends wait for their replies, so the space holds all three records before the queries run.
The records are persistent, so each query sees all of them.
The query `~ ([1, 1]; metric="dot")` takes `[3, 4]`, because its dot product 7 is the greatest.
The query with the threshold 0.9 takes `[0, 1]`, the only record whose cosine with `[0, 1]` is at least 0.9.

A record can also keep another payload and give its vector with the send param `embedding`.
A list query with `k` binds one list: the payload values of the first `k` eligible records, in the order of their scores.

```rholang
// A record can keep another payload and give its vector with the send param
// `embedding`. A list query with `k` binds the payloads of the first k
// eligible records, in the order of their scores. With `scores=true` each
// element is [score, value].
//
// The space uses the euclidean metric. The query keeps the records within the
// distance 5 of [0, 0], the nearest first.
new vdb(`rho:space:hashmap:vectordb`), out in {
  for (space <- vdb!?("default"; capacity=8, dimensions=2, metric="euclidean")) {
    space!?("produce", "docs", ["origin"]; embedding=[0, 0]);
    space!?("produce", "docs", ["far"]; embedding=[3, 4]);
    use space {
      for (@top <- @"docs" ~ ([0, 0]; radius=5, k=2, scores=true)) { out!(top) }
    }
  } |
  for (@list <- out) { @"rrs.example.vectordb.list"!(list) }
}
```

With `scores=true`, each element of the list is `[score, value]`, so the deploy reports `[[0.0, "origin"], [5.0, "far"]]`.
When the receive runs before the sends, it fires as soon as one record is eligible, with the eligible records of that moment.

### The keys of a query

| Key | Meaning |
|---|---|
| The query vector | The vector of the query. Without it, the receive reads the records in their canonical order. |
| `metric` | The metric of this query, instead of the metric of the space. |
| `threshold` | The least similarity, from 0 to 1, for `cosine`, `dot` and `jaccard`. |
| `radius` | The greatest distance, at least 0, for `euclidean`, `manhattan` and `hamming`. |
| `k` | List mode: the list of the first `k` eligible records. |
| `results` | List mode with `"all"`: the list of all eligible records. |
| `scores` | `true` delivers `[score, value]` for each record. |
| `ids` | Only the records whose ids are in this list, for example the ids that `rho:vectordb:ann` gives. |

`threshold` and `radius` exclude each other, and so do `k` and `results`.
A receive in list mode cannot take part in a join with other channels.
Only a linear receive `<-` takes a query in source text.

### Capacity and merges

A store into a collection that holds `capacity` records fails the deploy with `CapacityExceeded`.
Two parallel branches that store into one collection merge with no conflict.
So a merged collection can hold more than `capacity` records for a time.
While it does, every store fails until removals bring the count below `capacity` ([decision 71](../rspace/reified-rspaces/decision-register.md#decision-71)).

### Vector arithmetic

The system process `rho:lang:vector` adds, scales and multiplies vectors exactly, and it computes norms and distances.
[Chapter 10](10-system-channels.md#reified-vectors-and-vectordb-services) describes it, with the processes `rho:vectordb:embed` and `rho:vectordb:ann` of the ChromaDB mirror.

## Determinism

Every node computes the same result for a deploy, and the replay of a block gives the same result again.
A space never picks a record by the timing of threads or by the order of a list in memory.

```rholang
// A bag holds three records that match one receive. The space picks the
// record by the canonical order of decision 41 (the hash of the source of
// each record), not by the order of execution. So every node, and the replay
// of the block, takes the same record.
//
// The sends wait for the reply of the handle method produce, so all three
// records are stored before the receive runs.
new factory(`rho:space:hashmap:bag`), out in {
  for (bag <- factory!?("default")) {
    bag!?("produce", "pool", ["red"]);
    bag!?("produce", "pool", ["green"]);
    bag!?("produce", "pool", ["blue"]);
    use bag {
      for (@picked <- @"pool") { out!(picked) }
    }
  } |
  for (@picked <- out) { @"rrs.example.determinism"!(picked) }
}
```

The receive takes `"green"`, because `"green"` comes first in the canonical order of the three records.
The canonical order sorts the records by the hash of the send that stored each one ([decision 41](../rspace/reified-rspaces/decision-register.md#decision-41)).
So every node and every replay take `"green"`.

A larger example keeps a checkpoint of an account in a cell space, and it rolls back a transfer that would make the balance negative:

```rholang
// Speculative execution with a checkpoint record. An account lives in a cell
// space, which holds one record on each channel. A transfer keeps the old
// balance as a checkpoint and writes the new balance. A commit drops the
// checkpoint, and a rollback writes the checkpoint back.
//
// The deploy transfers 300, which commits. Then it transfers 900, which would
// make the balance negative, so it rolls back to 700.
new factory(`rho:space:hashmap:cell`), out in {
  for (cells <- factory!?("default")) {
    use cells {
      new account, saved in {
        account!(1000) |
        for (@balance <- account) {
          saved!(balance) | account!(balance - 300) |
          for (_ <- saved) {
            for (@after <- account) {
              out!(["committed", after]) |
              saved!(after) | account!(after - 900) |
              for (@candidate <- account) {
                if (candidate < 0) {
                  for (@restored <- saved) { account!(restored) | out!(["rolled back", restored]) }
                } else {
                  for (_ <- saved) { account!(candidate) | out!(["committed", candidate]) }
                }
              }
            }
          }
        }
      }
    }
  } |
  for (@step <= out) { @"rrs.example.speculative"!(step) }
}
```

## When a deploy aborts

A wrong use of a space aborts the deploy, and the node reverts every effect of the deploy.
Table 9 lists the codes that a program meets most often.
The [agent API](../rspace/reified-rspaces/agent-api.md#10-aborts) lists every code, and the table of all examples below names an example for each code that a program can reach.

*Table 9. Frequent aborts.*

| Code | Cause |
|---|---|
| `UnknownParam`, `InvalidParam` | A param that the family or the collection does not take, or a value outside its range |
| `QuerySpecUnsupported` | A query on a collection that takes no queries |
| `CrossSpaceJoin` | A join across spaces |
| `CellOccupied` | A second record or a second waiting receive in a cell |
| `OutOfNames`, `NotAnIndexName` | A full Array space, or a channel that is not an index name of an Array or a Vector space |
| `TheoryRefuted` | A value outside the theory of the space |
| `DanglingHandle`, `DanglingName` | A handle or a name of a temp or seq space whose deploy has ended |
| `SeqSent`, `SeqConcurrent` | A name of a seq space as data, or two uses of one channel of a seq space in parallel |
| `CapacityExceeded`, `DimensionMismatch` | A full VectorDB collection, or a vector with another length than the space |
| `PersistentFactoryCall`, `PersistentOpCall`, `PersistentHandleCall` | A persistent call `!!` of a factory, a system process or a handle |

A protocol error of a handle call does not abort the deploy.
The call gets a typed reply `("rho:space:error", code, detail)` instead, for example for an unknown method name.

## Writing your own space

A program can implement a space itself [[FIPS-RRS]](#ref-fips-rrs).
Any agent that answers three methods can serve as a space.
A program names it as it names a native space: in a `use` block or in an annotation `new x : a`.
The node translates the names, sends and receives of such a space into method calls on the agent ([agent API](../rspace/reified-rspaces/agent-api.md)).
Figure 3 shows the translation of a `new`, a send and a receive.

![Translation of a use block of a user agent A as a sequence. The deploy runs use A { new x in { x!(1) | for (@v <- x) { P } } }. For the new, the reducer draws the names of HEAD, substitutes the body and wraps it in USE(A), and computes the reply name r from base.split_byte(5).split_short(0). It produces A!(r, "gensym") and the resume receive on r. The agent replies a fresh name raw on r, and the resume receive binds x to bundle{(AGENT_TAG, A, raw)}. For the send x!(1), the reducer routes x by R4 to the agent A with raw and produces A!(sink, "produce", raw, [1]); the agent stores (raw, [1]) and offers it to its stored consumes, of which there are none. For the receive for (@v <- x) { P }, the reducer routes x to A, builds the matcher state with the pattern [@v] and no guard, puts a persistent receive on X(m) that forwards to MATCHER_OP and a receive on B(b) that is linear for the arrow <- and holds the body USE(A, P), and produces A!(sink, "consume", [(bundle+{m}, raw)], "<-", bundle+{b}). The agent calls m!(ret, [1]) for the stored message. The forwarder of m calls MATCHER_OP, which decodes the state of m, matches [1] against [@v] and checks the guard, and replies (true, {"0": 1}) on ret. The agent removes the message for the arrow <- and sends b!({"0": 1}), and P runs in the context of A with v = 1.](../rspace/reified-rspaces/diagrams/04-user-agent-translation.svg)

*Figure 3. Source: [diagrams/04-user-agent-translation.puml](../rspace/reified-rspaces/diagrams/04-user-agent-translation.puml), rendered by [diagrams/render.sh](../rspace/reified-rspaces/diagrams/render.sh).*

### The methods of a space agent

| Method | When the node calls it | What the agent does |
|---|---|---|
| `gensym()` | A `new` binds a name of the space. | It replies one fresh name, and the node binds the name of the agent with it. |
| `produce(raw, msg)` | A program sends on a name of the space. `raw` is the channel, and `msg` is the list of the data. | It stores the message, or it fires a stored consume. |
| `consume(pairs, arrow, body)` | A program receives on names of the space. `pairs` holds one pair `(matcher, raw)` for each bind. | It fires on stored messages, or it stores the consume. |

The node calls each method in the call shape of the Agents FIPS [[FIPS-AGENTS]](#ref-fips-agents): the return channel, the method name, and then the arguments.
The return channel of `produce` and `consume` is a sink that absorbs any reply.
A matcher `m` answers the call `m!(ret, msg)` with `(true, env)` when `msg` matches its bind and passes the guard of the bind, and with `(false, Nil)` otherwise.
The map `env` holds the values of the binders of the bind.
The body capability takes one map: the union of the maps `env` of all binds.
The arrow is `"<-"` for a linear receive, `"<<-"` for a peek and `"<="` for a persistent receive.

### The obligations of an agent

The node does not check the behavior of an agent.
The author of an agent keeps three obligations:

- **ASO-1.** After each method completes, no stored consume and stored messages exist such that every matcher of the consume accepts its message.
- **ASO-2.** For the arrows `"<-"` and `"<<-"`, the agent sends to the body capability at most once. `"<-"` removes the chosen messages, and `"<<-"` keeps them.
- **ASO-3.** The agent replies to each `gensym` call exactly once, with a fresh name.

An agent that breaks an obligation breaks the semantics of its space, but it never breaks consensus.
Each call is an ordinary event of the default space, so every node charges it and replays it in the same way.
For example, an agent that never answers leaves the receives of its space pending, and replay reproduces the pending receives.

### A wrapper agent

The simplest agent forwards each call to the handle of a native space.
The native handle accepts the matchers that the node built for the receive, so the forwarded `consume` works.

```rholang
// A user agent can serve a space by forwarding each call to the handle of a
// native space. In a use block of the agent, the node translates `new`, each
// send and each receive into the calls gensym, produce and consume of the
// agent. This agent forwards each call to a native bag space with the
// method-call sugar. The native handle accepts the matchers that the node
// built for the receive, runs the receive in its own space, and sends the
// bound values to the body capability of the receive.
new factory(`rho:space:hashmap:bag`), wrapper, out in {
  for (space <- factory!?("default")) {
    for (ret, @"gensym" <= wrapper) {
      for (name <- space!gensym()) { ret!(*name) }
    } |
    for (_, @"produce", @raw, @msg <= wrapper) {
      space!produce(raw, msg).
    } |
    for (_, @"consume", @pairs, @arrow, body <= wrapper) {
      space!consume(pairs, arrow, *body).
    } |
    use wrapper {
      new job in {
        job!("work") |
        for (@task <- job) { out!(task) }
      }
    }
  } |
  for (@task <- out) { @"rrs.example.wrapper"!(task) }
}
```

### The reference agent

The example `examples/reified_rspaces/agent_hashmap_bag.rho` is an agent of the Agents FIPS.
Each instance is a space with HashMap channels and bag collections, and the constructor takes the qualifier `"default"`.

```rholang
// The reference user agent of Reified RSpaces (P8, design v2 W8).
//
// agentHashmapBag is an agent of the Agents FIPS. Each instance is a space with HashMap
// channels and bag collections. A program names the space in a `use` block or in an annotation
// `new x : space`, and the node translates the names, sends and receives of the space into
// three method calls on the instance:
//
//   gensym()                     reply one fresh name
//   produce(raw, msg)            the send of the data list msg on the channel raw
//   consume(pairs, arrow, body)  a receive: one (matcher, raw) pair for each bind, the arrow
//                                "<-", "<<-" or "<=", and the body capability
//
// A matcher m answers m!(ret, msg) with (true, env) when msg matches its bind and passes its
// guard, and with (false, Nil) otherwise. The body capability takes the union of the env maps
// of the binds. The return channel of produce and consume is a sink that absorbs any reply.
//
// An instance keeps its state on one channel, so its methods run one at a time. The state is a
// map from each channel to its stored messages, in insertion order, and the list of the stored
// consumes, in insertion order. A produce offers the new message to the stored consumes in
// order, and a consume tries the stored messages at once. A peek keeps the message for the next
// stored consume, and a persistent consume fires until no stored messages satisfy it. So no
// stored consume and stored messages match when a method completes (ASO-1). A linear consume
// runs its body at most once (ASO-2), and gensym replies exactly once (ASO-3).
//
// The instance waits for the answer of each matcher while it holds its state. So a caller that
// brings a matcher that never answers stops the instance. Give the reference of a space only to
// programs that you trust.
new agentHashmapBag, offer, drain, find in {
  // offer: the stored messages hold a new message. From position k on, fire the first stored
  // consume that the stored messages satisfy. A fire takes the new message, because no stored
  // consume matched before it arrived. A peek keeps the message, so the next consumes get it.
  contract offer(@instance, @messages, @consumes, @k) = {
    if (k == consumes.length()) {
      @(instance, "state")!(messages, consumes)
    } else {
      match consumes.nth(k) {
        (pairs, arrow, body) => {
          new found in {
            find!(pairs, 0, 0, messages, {}, *found) |
            for (@result <- found) {
              match result {
                (true, left, env) => {
                  @body!(env) |
                  match [arrow, consumes.slice(0, k) ++ consumes.slice(k + 1, consumes.length())] {
                    ["<<-", others] => {
                      offer!(instance, messages, others, k)
                    }
                    ["<-", others] => {
                      @(instance, "state")!(left, others)
                    }
                    _ => {
                      @(instance, "state")!(left, consumes)
                    }
                  }
                }
                _ => {
                  offer!(instance, messages, consumes, k + 1)
                }
              }
            }
          }
        }
      }
    }
  } |

  // drain: fire a new consume on the stored messages, or store it. A persistent consume fires
  // again until the stored messages no longer satisfy it, and then it is stored.
  contract drain(@instance, @messages, @consumes, @pairs, @arrow, @body) = {
    new found in {
      find!(pairs, 0, 0, messages, {}, *found) |
      for (@result <- found) {
        match result {
          (true, left, env) => {
            @body!(env) |
            match arrow {
              "<<-" => {
                @(instance, "state")!(messages, consumes)
              }
              "<-" => {
                @(instance, "state")!(left, consumes)
              }
              _ => {
                drain!(instance, left, consumes, pairs, arrow, body)
              }
            }
          }
          _ => {
            @(instance, "state")!(messages, consumes ++ [(pairs, arrow, body)])
          }
        }
      }
    }
  } |

  // find: the first choice, in insertion order, of distinct stored messages for the binds of
  // pairs from bind i on, such that the matcher of each bind accepts its message. Bind i tries
  // its stored messages from position j on. A message that the matcher accepts leaves the
  // stored messages for the later binds, so a join that names one channel twice takes two
  // distinct messages. The answer is (true, left, env), with the stored messages that remain
  // and the union of the env maps, or (false, Nil, Nil).
  contract find(@pairs, @i, @j, @messages, @env, ret) = {
    if (i == pairs.length()) {
      ret!((true, messages, env))
    } else {
      match pairs.nth(i) {
        (matcher, channel) => {
          match messages.getOrElse(channel, []) {
            stored => {
              if (j == stored.length()) {
                ret!((false, Nil, Nil))
              } else {
                new answer, deeper in {
                  @matcher!(*answer, stored.nth(j)) |
                  for (@reply <- answer) {
                    match reply {
                      (true, bound) => {
                        match stored.slice(0, j) ++ stored.slice(j + 1, stored.length()) {
                          [] => {
                            find!(pairs, i + 1, 0, messages.delete(channel), env.union(bound), *deeper)
                          }
                          rest => {
                            find!(pairs, i + 1, 0, messages.set(channel, rest), env.union(bound), *deeper)
                          }
                        } |
                        for (@result <- deeper) {
                          match result {
                            (true, _, _) => {
                              ret!(result)
                            }
                            _ => {
                              find!(pairs, i, j + 1, messages, env, *ret)
                            }
                          }
                        }
                      }
                      _ => {
                        find!(pairs, i, j + 1, messages, env, *ret)
                      }
                    }
                  }
                }
              }
            }
          }
        }
        _ => {
          ret!((false, Nil, Nil))
        }
      }
    }
  } |

  agent agentHashmapBag {
    constructor(@"default") {
      @(*private, "state")!({}, [])
    } |
    method gensym() {
      new raw in { return!(*raw) }
    } |
    method produce(@raw, @msg) {
      for (@messages, @consumes <- @(*private, "state")) {
        offer!(*private, messages.set(raw, messages.getOrElse(raw, []) ++ [msg]), consumes, 0)
      }
    } |
    method consume(@pairs, @arrow, @body) {
      match [pairs, arrow] {
        [[_, ..._], "<-" \/ "<<-" \/ "<="] => {
          for (@messages, @consumes <- @(*private, "state")) {
            drain!(*private, messages, consumes, pairs, arrow, body)
          }
        }
        _ => {
          Nil
        }
      }
    } |
    default(...@args) {
      Nil
    }
  } |

  // Demo: a space of the agent in a use block. A send and a receive on a name of the space meet
  // through the agent, and the sum of the received values reaches a public channel.
  new out in {
    for (space <- agentHashmapBag!?("default")) {
      use space {
        new x in {
          x!(1, 2) |
          for (@a, @b <- x) { out!(a + b) }
        }
      }
    } |
    for (@sum <- out) { @"agentHashmapBag.demo"!(sum) }
  }
}
```

An instance keeps its state on one channel, so the methods of one instance run one at a time.
The state maps each channel to its stored messages, and it lists the stored consumes, both in insertion order.

A `produce` stores the message, and the contract `offer` gives it to the stored consumes in insertion order.
A `consume` runs the contract `drain`, which fires at once on the stored messages or stores the consume.
The contract `find` tries the stored messages of each bind in insertion order and calls one matcher at a time.
A message that a matcher accepts leaves the candidates of the later binds, so a join that names one channel twice takes two distinct messages.
A fire keeps the chosen messages only for `"<<-"`, and it keeps the consume only for `"<="` (ASO-2).
A persistent consume fires again until the stored messages no longer satisfy it, so no match stays pending (ASO-1).

### Cost

Each operation on an agent space runs ordinary COMMs in the default space.
A send makes one call of the agent.
A receive makes one call, and it installs one matcher receive for each bind and one body receive.
Each matcher call costs three COMMs: the call, the forwarder hop to the matcher process, and the reply.
So an operation on an agent space costs more than the same operation on a native space, whose handle runs the match itself.

### Trust

An agent sends the values of a receive to the body capability.
So a program trusts the agent to send the values that the matchers gave.
A native space accepts only the matchers that the node built, but an agent calls the matchers that a consume brings.
The reference agent waits for the answer of each matcher while it holds its state, so a matcher that never answers stops the instance.
Give the reference of an agent space only to programs that you trust.
The call `getSpaceAgent` on an agent space replies `NotANativeSpace`, because the node does not know the constructor of an agent.

### The conformance kit

The test suite `rholang/tests/reified_agent_conformance.rs` checks an agent through programs that use the agent as a space.
The kit runs each case as a sequence of deploys, and it compares all outputs after each deploy.
So a match that stays pending fails the case (ASO-1).
The cases cover `gensym`, the three arrows, a join, a guard, a join over one channel twice, and two waiting receives.

The kit runs each case against the reference agent and against a wrapper agent that forwards each call to a native `rho:space:hashmap:bag` space.
The negative control is a faulty agent whose `consume` stores each receive without a try.
The kit fails at the step that leaves its match pending.
Another agent joins the kit with one value `AgentUnderTest`, which holds the deploy that creates the agent.

## The FIPS example

The Reifying RSpaces FIPS ends with an example that uses most features at once [[FIPS-RRS]](#ref-fips-rrs).
This file runs it with concrete names.

```rholang
// The example code of the Reifying RSpaces FIPS, with concrete names. Two
// spaces: a bag space with the theory Nat, and a temp PathMap space. In a use
// block of the first space, `new` makes two names in each space, and a forged
// name lands in the first space. A join takes two channels of one space, and
// a second join takes two channels of the other space. getSpaceAgent gives the
// factory of a space, and assertEqual compares it with the factory.
//
// The calls of getSpaceAgent and assertEqual stand outside the use block. A
// sync call `x!?(...)` creates its reply name with `new`, so inside the block
// the reply name would belong to space_1, whose theory Nat refuses the reply.
new HMB(`rho:space:HashMapBagSpace`),
    PM(`rho:space:PathMapSpace`),
    getSpaceAgent(`rho:space:getSpaceAgent`),
    assertEqual(`rho:assert:assertEqual`), out in {
  for (space_1 <- HMB!?("default", free Nat()) ; space_2 <- PM!?("temp", free Any())) {
    use space_1 {
      new x_1 : space_1, x_2 : space_1, x_3 : space_2, x_4 : space_2 in {
        x_1!(1) | x_2!(2) | x_3!(3) | x_4!(4) |
        @{*x_1 | *x_2}!(5) |
        for (@a <- x_1 & @b <- x_2 ; @c <- x_3 & @d <- x_4) {
          out!(["joins", a + b, c + d])
        } |
        for (@forged <- @{*x_1 | *x_2}) { out!(["forged", forged]) }
      }
    } |
    for (@capability <- getSpaceAgent!?(*space_1)) {
      for (@same <- assertEqual!?(capability, *HMB)) { out!(["agent", same]) }
    }
  } |
  for (@answer <= out) { @"rrs.example.fips"!(answer) }
}
```

The join of `x_1` and `x_2` gives 3, and the join of `x_3` and `x_4` gives 7.
The forged name `@{*x_1 | *x_2}` routes to `space_1`, because the `use` block of `space_1` holds it.
`getSpaceAgent` gives the factory of `space_1`, which is the capability that the URN `rho:space:HashMapBagSpace` binds.

## All examples

| Example | Files | What it shows |
|---|---|---|
| `queue-fifo` | `queue_fifo.rho` | A queue space gives back its records in arrival order (FIFO). |
| `queue-rejects-a-send-param` | `queue_rejects_a_send_param.rho` | A send param on a queue collection aborts with UnknownParam. |
| `stack-lifo` | `stack_lifo.rho` | A stack space gives back its records in reverse arrival order (LIFO). |
| `stack-rejects-a-query` | `stack_rejects_a_query.rho` | A query on a stack aborts with QuerySpecUnsupported. |
| `set-idempotent` | `set_idempotent.rho` | A set space holds each value once, and a receive retires the equal records. |
| `set-rejects-a-send-param` | `set_rejects_a_send_param.rho` | A send param on a set collection aborts with UnknownParam. |
| `cell-exactly-once` | `cell_exactly_once.rho` | A cell holds at most one record on each channel. |
| `cell-occupied` | `cell_occupied.rho` | A second send to a full cell aborts with CellOccupied. |
| `bag-basics` | `bag_basics.rho` | A bag space takes any matching record, as the default space does. |
| `bag-rejects-a-query` | `bag_rejects_a_query.rho` | A query on a bag aborts with QuerySpecUnsupported. |
| `priority-queue` | `priority_queue.rho` | A priority queue serves the highest level first, and a receive can take one level. |
| `priority-queue-rejects-a-level` | `priority_queue_rejects_a_level.rho` | A level beyond the bound `priorities` aborts with InvalidParam. |
| `pathmap-prefix-aggregation` | `pathmap_prefix_aggregation.rho` | The PathMap example of the FIPS: a receive on a prefix takes re-rooted records. |
| `pathmap-hierarchical-spaces` | `pathmap_hierarchical_spaces.rho` | A PathMap space as a tree of channels. |
| `array-indexed-space` | `array_indexed_space.rho` | An Array space hands out a fixed number of names. |
| `array-out-of-names` | `array_out_of_names.rho` | A name beyond the size of an Array space aborts with OutOfNames. |
| `vector-indexed-space` | `vector_indexed_space.rho` | A Vector space grows with each new name. |
| `hashset-dedup-spaces` | `hashset_dedup_spaces.rho` | HashSet spaces hold units, with the set and the bag collections. |
| `hashset-rejects-a-payload` | `hashset_rejects_a_payload.rho` | A send with data to a HashSet space aborts with PayloadNotEmpty. |
| `hashset-rejects-a-pattern` | `hashset_rejects_a_pattern.rho` | A receive with a binder on a HashSet collection aborts with PatternNotEmpty. |
| `space-qualifier-default` | `space_qualifier_default_1.rho`, `space_qualifier_default_2.rho` | A default space persists, so a later deploy reads its records through its handle. |
| `space-qualifier-temp` | `space_qualifier_temp.rho` | A temp space works like any other space within its deploy. |
| `space-qualifier-seq` | `space_qualifier_seq.rho` | A seq space takes uses of a channel one after the other. |
| `space-qualifiers-seq-sent` | `space_qualifiers_seq_sent.rho` | A name of a seq space sent as data aborts with SeqSent. |
| `seq-rejects-concurrent-uses` | `seq_rejects_concurrent_uses.rho` | A send and a receive in parallel on one channel of a seq space abort with SeqConcurrent. |
| `theory-validated-space` | `theory_validated_space.rho` | The builtin theory Nat admits natural numbers. |
| `theory-refuses-a-value` | `theory_refuses_a_value.rho` | The theory Nat refuses -1 with TheoryRefuted. |
| `get-space-agent` | `get_space_agent.rho` | getSpaceAgent gives the factory of a space, and assertEqual compares values. |
| `vectordb-queries` | `vectordb_queries.rho` | A VectorDB query takes the most similar record. |
| `vectordb-list-scores` | `vectordb_list_scores.rho` | A list query with k, a radius and scores. |
| `vectordb-multi-topic` | `vectordb_multi_topic.rho` | Two topics in two VectorDB spaces. |
| `vectordb-dimension-mismatch` | `vectordb_dimension_mismatch.rho` | A vector of the wrong length aborts with DimensionMismatch. |
| `vectordb-capacity-exceeded` | `vectordb_capacity_exceeded.rho` | A store into a full VectorDB collection aborts with CapacityExceeded. |
| `vector-ops` | `vector_ops.rho` | The five exact operations of rho:lang:vector. |
| `determinism-demo` | `determinism_demo.rho` | The canonical order picks one record on every node and in replay. |
| `multi-space-isolation` | `multi_space_isolation.rho` | Forged names in two spaces are two channels. |
| `cross-space-join` | `cross_space_join.rho` | A join across two spaces aborts with CrossSpaceJoin. |
| `space-annotations` | `space_annotations.rho` | new x : s creates a name in the space s. |
| `handle-methods` | `handle_methods.rho` | The handle methods with the method-call sugar of the Agents FIPS. |
| `factory-rejects-a-bad-argument` | `factory_rejects_a_bad_argument.rho` | A constructor argument that is not a qualifier, a theory or a params section aborts with BadConstructorArgument. |
| `factory-rejects-an-unknown-theory` | `factory_rejects_an_unknown_theory.rho` | A theory that is not a builtin theory aborts the factory call with UnknownTheory. |
| `factory-rejects-an-unknown-param` | `factory_rejects_an_unknown_param.rho` | A named param outside the schema of the family aborts the factory call with UnknownParam. |
| `factory-rejects-a-missing-param` | `factory_rejects_a_missing_param.rho` | A factory call without a required param aborts with MissingParam. |
| `factory-rejects-an-invalid-param` | `factory_rejects_an_invalid_param.rho` | A factory param with a value outside its range aborts with InvalidParam. |
| `factory-rejects-a-positional-param` | `factory_rejects_a_positional_param.rho` | A positional param aborts the factory call with PositionalParamsUnsupported. |
| `factory-rejects-a-forged-params-tuple` | `factory_rejects_a_forged_params_tuple.rho` | A params tuple that a program builds with a marker from its own quoted code, in a form that the normalizer never writes, aborts with Malformed. |
| `factory-rejects-a-persistent-call` | `factory_rejects_a_persistent_call.rho` | A persistent send to a factory aborts with PersistentFactoryCall. |
| `system-op-rejects-a-persistent-call` | `system_op_rejects_a_persistent_call.rho` | A persistent send to assertEqual aborts with PersistentOpCall. |
| `system-op-rejects-params` | `system_op_rejects_params.rho` | A params section on a call of assertEqual aborts with ParamsUnsupported. |
| `handle-rejects-a-persistent-call` | `handle_rejects_a_persistent_call.rho` | A persistent send to the handle of a space aborts with PersistentHandleCall. |
| `temp-space-ends-with-its-deploy` | `temp_space_ends_with_its_deploy_1.rho`, `temp_space_ends_with_its_deploy_2.rho` | A temp space ends with its deploy, so a later call aborts with DanglingHandle. |
| `fips-example` | `fips_example.rho` | The example code of the Reifying RSpaces FIPS with concrete names. |
| `speculative-execution` | `speculative_execution.rho` | A checkpoint record with commit and rollback in a cell space. |
| `vectordb-embed` | `vectordb_embed.rho` | The recorded process rho:vectordb:embed, with the fake embedder of the test support. |
| `vectordb-mirror` | `vectordb_mirror_1.rho`, `vectordb_mirror_2.rho` | The ChromaDB mirror of a VectorDB space: one deploy stores records, and a later deploy asks rho:vectordb:ann and lets the exact engine select. |
| `agent-hashmap-bag` | `agent_hashmap_bag.rho` | The reference user agent: an agent of the Agents FIPS whose instances are spaces with HashMap channels and bag collections. |
| `agent-wrapper-forwards-to-native` | `agent_wrapper_forwards_to_native.rho` | A user agent that forwards gensym, produce and consume to the handle of a native space with the method-call sugar. |
| `agent-space-rejects-a-persistent-send` | `agent_space_rejects_a_persistent_send.rho` | A persistent send in a use block of a user agent aborts with PersistentSendOnAgentSpace. |
| `agent-space-rejects-params` | `agent_space_rejects_params.rho` | A send with a params section in a use block of a user agent aborts with ParamsUnsupportedOnAgentSpace. |
| `agent-space-rejects-a-query` | `agent_space_rejects_a_query.rho` | A receive with a query in a use block of a user agent aborts with QuerySpecUnsupportedOnAgentSpace. |
| `agent-space-rejects-a-spanning-guard` | `agent_space_rejects_a_spanning_guard.rho` | A guard over the binders of two binds of a receive on a user agent aborts with AgentGuardSpansJoin. |
| `agent-space-rejects-a-deep-pattern` | `agent_space_rejects_a_deep_pattern.rho` | A pattern of 30 nested lists in a receive on a user agent aborts with AgentPatternTooDeep. |
| `agent-capability-rejects-a-persistent-call` | `agent_capability_rejects_a_persistent_call.rho` | A persistent send on the body capability of a consume call aborts with PersistentCapabilityCall. |

## References

<a id="ref-fips-rrs"></a>[FIPS-RRS] M. Stay and L. G. Meredith, "Reifying RSpaces," F1r3fly Improvement Proposal, 2025-09-26. [FIPS repository, approved/2025-09-26-Reifying-RSpaces](https://github.com/F1R3FLY-io/FIPS/blob/main/approved/2025-09-26-Reifying-RSpaces/Reifying%20RSpaces.md)

<a id="ref-fips-agents"></a>[FIPS-AGENTS] M. Stay, "Agents," F1r3fly Improvement Proposal, 2025-08-20. [FIPS repository, approved/2025-08-20-Agents](https://github.com/F1R3FLY-io/FIPS/blob/main/approved/2025-08-20-Agents/Agents.md)
