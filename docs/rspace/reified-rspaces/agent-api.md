# Reified RSpaces agent API

This document states the interface between a Rholang program and the native spaces of Reified RSpaces.
It gives each call that a program can make, the reply of the call, and each error with its exact form.
It covers every native space family and the [user agent-backed spaces](glossary.md#user-agent-backed-space).
Each `rholang` block of this document is the text of one file of `examples/reified_rspaces/`, and the examples harness runs each file.

The [specification](specification.md) states the behavior of a space after a call reaches it.
The [design](design.md#n7-factories-and-handles) explains how the code implements each call.
The [decision register](decision-register.md) is normative. Where this document and the register differ, the register applies.

## Contents

- [Terms](#terms)
- [1. Overview](#1-overview)
- [2. System processes](#2-system-processes)
- [3. Factories](#3-factories)
- [4. Handles](#4-handles)
- [5. Handle methods](#5-handle-methods)
- [6. Replies](#6-replies)
- [7. Typed replies](#7-typed-replies)
- [8. getSpaceAgent](#8-getspaceagent)
- [9. assertEqual](#9-assertequal)
- [10. Aborts](#10-aborts)
- [11. Limits](#11-limits)
- [12. User agent-backed spaces](#12-user-agent-backed-spaces)
- [References](#references)

## Terms

The [glossary](glossary.md) defines each linked term.
Table 1 defines the other terms of this document.

*Table 1. The terms of the agent API.*

| Term | Definition |
|---|---|
| Call | A send of a program to a [system process](glossary.md#system-process): `p!(ret, a…)`. The sugar `p!?(a…)` makes the reply name `ret` and binds the reply. |
| Reply name | The first argument `ret` of a call. The process sends its reply on this name. |
| Factory capability | The value `bundle+{[0xE0, i]}` that the URN of the family with the index `i` binds. A call on it reaches the [factory](glossary.md#factory). |
| Constructor | The arguments of a factory call after the reply name. |
| [Handle capability](glossary.md#handle-capability) | The value `bundle+{H(sid)}` that a factory replies. A call on it reaches the space. |
| `this` | The channel $`H(\mathit{sid})`$ of a space, which is the body of its handle capability. |
| Method | The name of an operation of a handle, as a string, for example `"produce"`. |
| [Typed reply](glossary.md#typed-reply) | The reply `("rho:space:error", code, detail)` of a call with a protocol error. |
| Abort | A failure of the deploy with `InterpreterError::ReduceError("reified:<Code>: <message>")`. The deploy reverts all its effects. |
| Levenshtein distance | The least number of insertions, deletions, and substitutions of one character that change one word into another [[Lev66](#ref-lev66)]. |
| Suggestion | The candidate name at the least Levenshtein distance from a wrong name, when that distance is at most 2. A tie goes to the bytewise least candidate. |

## 1. Overview

A program uses a native space in four steps:

1. Bind the factory URN with `new`, for example ``new hmq(`rho:space:hashmap:queue`) in { … }``.
2. Call the factory with a reply name and a constructor. The factory creates the space and replies its handle capability.
3. Send and receive on the space, inside a `use` block or on names that `new x : s` puts in the space.
4. Call a method of the handle, for example `h!(ret, "produce", ch, [1])`, and receive the reply on `ret`.

A receive can join two or more channels of one space, for example `for (@a <- x & @b <- y) { P }` inside `use s { … }` ([specification 3.8](specification.md#38-joins)).
The join engine of the space runs such a receive ([design N12](design.md#n12-wave-w3-of-p5-joins-of-one-space)).
A join over two spaces, or over a space and the default space, aborts with `CrossSpaceJoin`.

Figure 1 shows the creation of a space and one handle call, from the send of the program to the reply.

![Lifecycle of a native handle as a sequence. A deploy sends the factory call hmq!(*ret) with its constructor. The reducer routes the op channel [0xE0, 0x01] to the default space, checks the persistent flag, packs the constructor and produces on the op channel. The COMM with the installed factory runs FactoryProcess::call: it validates the constructor and derives sid from the random state of the call. One space job runs one creation transaction: begin on C(sid), S(sid) and H(sid), a check for SpaceAlreadyExists, the config record stored persistently on C(sid), the space counter S(sid) = 0, and the handle continuation parked persistently on H(sid); the commit logs the events. The factory replies bundle+{H(sid)} on ret. Later the deploy calls h!(*ack, "produce", "jobs", [1]): the reducer removes the write bundle, routes H(sid) to the default space and produces on it; the COMM with the persistent handle continuation runs its body, which produces (this, args) on HANDLE_OP. HANDLE_OP checks the bare handle id, reads C(sid) in a read-only space job that commits nothing, parses the message, and runs the method produce as a collection operation in its own space job: the counter bump and the head-only offer. It dispatches the fires and replies Nil on ack. A protocol error replies a typed error ("rho:space:error", code, detail) instead, and a missing config record aborts with DanglingHandle.](diagrams/03-native-handle-lifecycle.svg)

*Figure 1. Source: [diagrams/03-native-handle-lifecycle.puml](diagrams/03-native-handle-lifecycle.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

## 2. System processes

The node installs the system processes of Tables 2, 2a, 2b and 2c (`reified/system_processes.rs:129-220`).
Each one listens on an [op channel](glossary.md#op-channel) $`[\mathtt{0xE0}, \mathit{op}]`$ and has the body reference $`200 + \mathit{op}`$.
The URN map binds each URN to the write bundle of its op channel. So a program can call each process, but it cannot receive on its channel.

*Table 2. The first factory and the processes of every space (`reified/system_channels.rs:62-95`).*

| URN | Op channel | Arity | Body reference | Call | Reply |
|---|---|---|---|---|---|
| `rho:space:hashmap:queue` | `[0xE0, 0x01]` | 2 | 201 | `hmq!(ret, ctor…)` | The handle capability of a new space |
| `rho:space:getSpaceAgent` | `[0xE0, 0x21]` | 2 | 233 | `getSpaceAgent!(ret, s)` | The factory capability of the family of `s`, or a typed reply |
| `rho:assert:assertEqual` | `[0xE0, 0x22]` | 3 | 234 | `assertEqual!(ret, a, b)` | A Boolean |
| `rho:reified:internal:handleOp` | `[0xE0, 0x30]` | 2 | 248 | `(this, message)`, from the handle continuation of a space | The reply of the method, or a typed reply |

*Table 2a. The other factories (`reified/system_channels.rs:386-412`).*

| URN | Op channel | Arity | Body reference | Constructor params |
|---|---|---|---|---|
| `rho:space:hashmap:bag` | `[0xE0, 0x00]` | 2 | 200 | none |
| `rho:space:hashmap:stack` | `[0xE0, 0x02]` | 2 | 202 | none |
| `rho:space:hashmap:set` | `[0xE0, 0x03]` | 2 | 203 | none |
| `rho:space:hashmap:cell` | `[0xE0, 0x04]` | 2 | 204 | none |
| `rho:space:hashmap:priorityqueue` | `[0xE0, 0x05]` | 2 | 205 | `priorities` |
| `rho:space:hashmap:vectordb` | `[0xE0, 0x06]` | 2 | 206 | `capacity` and `dimensions` (required), `metric`, `mirror` |
| `rho:space:pathmap:<inner>` | `[0xE0, 0x07]` to `[0xE0, 0x0C]` | 2 | 207 to 212 | `priorities` for a priority queue |
| `rho:space:array:<inner>` | `[0xE0, 0x0D]` to `[0xE0, 0x12]` | 2 | 213 to 218 | `size` (required), `cyclic`, and `priorities` for a priority queue |
| `rho:space:vector:<inner>` | `[0xE0, 0x13]` to `[0xE0, 0x18]` | 2 | 219 to 224 | `priorities` for a priority queue |
| `rho:space:vector:vectordb` | `[0xE0, 0x19]` | 2 | 225 | `capacity` and `dimensions` (required), `metric`, `mirror` |
| `rho:space:hashset:<inner>` | `[0xE0, 0x1A]` to `[0xE0, 0x1F]` | 2 | 226 to 231 | `priorities` for a priority queue. The qualifier must be `"seq"`. |
| `rho:space:HashMapBagSpace` | `[0xE0, 0x00]` | 2 | 200 | none: the alias of `hashmap:bag` |
| `rho:space:PathMapSpace` | `[0xE0, 0x07]` | 2 | 207 | none: the alias of `pathmap:bag` |

A row of Table 2a is active only when its family is registered, which is exactly when the family dispatch has its arm ([specification FA-1](specification.md#15-families)).
Every row of Table 2a is active.

Three rules hold for every process of Table 2 and Table 2a:

- **Fixed arity.** A process has a fixed arity ([decision I-18](decision-register.md#i-18)). A call with another arity never reaches the process, and its datum stays on the op channel.
- **No persistent call.** A persistent send `!!` to an op channel aborts the deploy before the call ([decision 32](decision-register.md#decision-32), item A21). The code is `PersistentFactoryCall` for a factory and `PersistentOpCall` for the other processes.
- **No params.** Only a factory and a handle take params. A send with params to `getSpaceAgent` or `assertEqual` aborts with `ParamsUnsupported`.

A URN that names no family has no binding.
A program that names such a URN fails with the baseline error `No value set for <urn>`, before any space operation.

*Table 2c. The processes of the VectorDB family (`reified/system_channels.rs:430-493`).*

| URN | Op channel | Arity | Body reference | Call | Reply |
|---|---|---|---|---|---|
| `rho:lang:vector` | `[0xE0, 0x23]` | 2 | 235 | `vec!(ret, op, x…)` | `[true, value]` or `[false, code, detail]` |
| `rho:vectordb:embed` | `[0xE0, 0x24]` | 2 | 236 | `embed!(ret, text)` | `[true, vector, model]` or `[false, code, message]` |
| `rho:vectordb:ann` | `[0xE0, 0x25]` | 5 | 237 | `ann!(ret, space, channel, query, options)` | `[true, ids, distances]` or `[false, code, message]` |

[Chapter 10](../../rholang/10-system-channels.md#reified-vectors-and-vectordb-services) gives an example of each process, and the [specification](specification.md#9-system-processes-of-the-vectordb-family) states their rules.
Only a build with the feature `chromadb` registers `embed` and `ann` ([decision 113](decision-register.md#decision-113)).
The three rules of Table 2 hold for `rho:lang:vector`, and the rules of fixed arity and no params hold for `embed` and `ann`.
A persistent send to `embed` or `ann` passes A21.
Base rule P2 of the recorded external services fails it at its fire with `SystemProcessShapeError` ([decision 106](decision-register.md#decision-106)).
A failure reply of `embed` or `ann` is the base reply `[false, code, message]`, with the code `EXT_BAD_ARG`, `EXT_FAILED` or `EXT_OUTPUT_TOO_LARGE` ([decision 105](decision-register.md#decision-105)).

*Table 2b. The internal processes of P8 (`reified/system_channels.rs:560-584`).*

| URN | Op channel | Arity | Body reference | Call | Reply |
|---|---|---|---|---|---|
| `rho:reified:internal:matcherOp` | `[0xE0, 0x31]` | 2 | 249 | `(m, [ret, proc])`, from the forwarder of a matcher capability | `(true, env)` or `(false, Nil)` on `ret` |
| `rho:reified:internal:sinkOp` | `[0xE0, 0x32]` | 0 and a remainder | 250 | Any message, as the return channel of a translated call | None |

Both processes serve the spaces that user agents implement ([decision 77](decision-register.md#decision-77)).
The rules of the persistent call and of params hold for them, and the sink takes a message of any arity (item SK-1).

The translation of a receive gives the agent one matcher capability `bundle+{m}` for each bind and one body capability `bundle+{b}` ([design N5](design.md#n5-routing-and-scope)).
The agent tests a stored message with the call `m!(ret, proc)`, where `proc` is the list of the values of the message.
The matcher process matches the elements of `proc` against the patterns of the bind of `m`, with the matcher of the host.
Then it checks the guard of `m`.
The reply `(true, env)` maps the decimal global position of each binder of the bind to its value, and `(false, Nil)` means no match.
When every bind has a match, the agent sends the union of the maps to the body capability with `b!(env)`.

Then the body of the receive runs with those values, as the body of a native receive does.

The translation of a `new` calls `A!(r, "gensym")` for each binder of the agent `A` (P8 step 4).
The agent replies one raw name on the reply name `r`, and the binder gets the agent-minted name of that raw name.
The sends and the receives on an agent-minted name then become the calls `produce` and `consume` on the agent.

## 3. Factories

### 3.1 The call

A factory call is a send on the factory capability: `hmq!(ret, a1, …, an)`.
The send hook packs the arguments after `ret` into one list, so the factory receives `[ret, [a1, …, an]]` (`reified/reducer_hooks.rs:1998-2038`).
So every factory has the fixed arity 2, and a constructor can have any number of elements.
The params section of the call stays in the list as its last element.

The constructor must be the canonical sequence $`[q?]\,[\mathrm{FREE}?]\,[\mathrm{PARAMS}?]`$ (`models/src/rust/space_factory_urn.rs:485-522`):

- $`q`$ is a string that names a qualifier: `"default"`, `"temp"`, or `"seq"`.
- $`\mathrm{FREE}`$ is a theory reference `free Th(args)`.
- $`\mathrm{PARAMS}`$ is the params section of the call, with named params only.

### 3.2 The gates

The factory checks the constructor with nine gates, in the order of Table 3 (`spaces/factory/constructor.rs:115-213`).
The gates have the numbers of rule FA-6 of the [specification](specification.md#15-families).
The first gate that fails aborts the deploy with its code, and the later gates do not run.

*Table 3. The gates of a factory call.*

| # | Check | Code of the abort |
|---|---|---|
| 1 | The family has a constructor schema. | `FamilyUnavailable` |
| 2 | The constructor is the canonical sequence. An unknown qualifier string, a malformed theory or params value, and any other element fail here. | `BadConstructorArgument` |
| 3 | The outer store takes the qualifier: the HashSet store takes only `"seq"`, and every other store takes every qualifier. | `BadConstructorArgument` |
| 4 | A theory names a builtin, `Nat`, `Int`, `String`, `Bool`, or `Any`, without arguments ([specification 5](specification.md#5-theories), rule TH-4). A builtin with arguments fails with `BadConstructorArgument`. | `UnknownTheory`, `BadConstructorArgument` |
| 5 | The params are named only. The send hook already checks this rule. | `PositionalParamsUnsupported` |
| 6 | Each named param is in the schema of the family, in the bytewise order of the keys. | `UnknownParam` |
| 7 | Each required param is present, for example the `size` of an Array space. | `MissingParam` |
| 8 | Each param value is in the range of its rule, in the bytewise order of the keys. | `InvalidParam` |
| 9 | A space with `mirror` has the qualifier `"default"` and a metric that ChromaDB indexes ([specification 3.9](specification.md#39-vectordb), rule VD-1). | `InvalidParam` |

The code `UnknownParam` of a factory has a suggestion in its message when a schema key is at most two edits away.
For example, the constructor `("default", "seq", "temp")` fails at gate 2.
The next program names the theory `Bounded`, so it fails at gate 4 with `UnknownTheory`:

```rholang
// The builtin theories are Nat, Int, String, Bool and Any. A factory call that
// names another theory aborts the deploy with UnknownTheory.
new factory(`rho:space:hashmap:bag`) in {
  for (bag <- factory!?("default", free Bounded())) { Nil }
}
```

The name `Natt` fails at gate 4 too, and its message suggests `Nat`.
Section [10](#10-aborts) gives an example of each gate that a program can reach.

### 3.3 The creation and the reply

After the gates, the factory derives the space id from the random state $`r`$ of the call ([decision I-19](decision-register.md#i-19)):

```math
\mathit{sid} = \mathrm{next}\bigl(\mathrm{split}_{\mathtt{0x01}}(\mathrm{split}_{\mathtt{0xE0}}(r))\bigr)
```

One space job then creates the space in one transaction.
It stores the config record on $`C(\mathit{sid})`$, the space counter $`S(\mathit{sid}) = 0`$, and the handle continuation on $`H(\mathit{sid})`$.
When $`C(\mathit{sid})`$ already holds a config record, the creation aborts the deploy with `SpaceAlreadyExists` and changes nothing.

After the commit, the factory sends the handle capability `bundle+{H(sid)}` on `ret`.
The factory sends this reply on `ret` without a check of `ret`.
So use a name from `new` as the reply name, because a forged reply channel such as `@"out"` routes to the default space (rule R7).

## 4. Handles

### 4.1 The handle capability

The handle capability `bundle+{H(sid)}` is a bundle with the write flag and without the read flag.
So a program can send calls with it, but it can neither receive on $`H(\mathit{sid})`$ nor open the bundle with a pattern.
A program that holds the capability can call the space, and it can give the capability to another process like any other name.

A call on a handle has the form `h!(ret, method, a…; k=v…)`.
The reducer removes the write bundle, and the channel $`H(\mathit{sid})`$ routes to the default space (rule R1).
The handle continuation of the space is a persistent receive on $`H(\mathit{sid})`$ with the pattern `[...@args]`, so it binds the whole call as one list.
Its body `[0xE0, 0x30]!(this, args)` forwards the call to `HANDLE_OP` (`reified/factory.rs:144-169`).
The forwarder has no guard and no cost authority, so the caller pays for each call.

The send hook applies two rules to a handle call before the forward:

- A persistent send `h!!(…)` aborts with `PersistentHandleCall`.
- A positional param aborts with `PositionalParamsUnsupported`. The named params stay in the call for `HANDLE_OP`.

### 4.2 The bare-handle rule

`HANDLE_OP` accepts a call only when `this` is the bare id $`H(\mathit{sid})`$ ([decision 32](decision-register.md#decision-32), item A6).
The bare id is a `GPrivate` of exactly 34 bytes with the KIND H or h.
It drops every other call, with no reply and no abort.
Only the forwarding receive of the space passes the bare id, and a program holds only the bundle.
So a program that binds the URN `rho:reified:internal:handleOp` gains no capability, because it cannot name a bare handle id.

### 4.3 The steps of a call

`HANDLE_OP` processes a call `(this, message)` in this order (`reified/handle.rs:240-410`):

1. When `this` is not a bare handle id, the process drops the call.
2. The process reads the config record in a read-only space job, which commits nothing. A missing record aborts with `DanglingHandle`, also when the message is malformed.
3. The process applies the [drop rule](#44-the-drop-rule-and-the-name-rule) to the message. A dropped message gets no reply.
4. A params value that is not canonical gives the typed reply `MalformedMessage`. The send hook checks the params of a program call first (`check_send_params`), and it aborts with `Malformed` on such a value.
5. A positional param aborts with `PositionalParamsUnsupported`.
6. A method name that is not one of the five methods gives the typed reply `UnknownMethod`.
7. An argument count that is not the arity of the method gives the typed reply `BadArgument` with the argument `"arguments"`.
8. A named param outside the schema of the method gives the typed reply `UnknownParam`. The process checks the keys in bytewise order.
9. The method checks its arguments and runs. It sends its reply on `ret`. The reply of `produce` is the last branch of the operation, after the branches of its fires (see below).

### 4.4 The drop rule and the name rule

`HANDLE_OP` drops a message when any of these conditions holds ([decision 34](decision-register.md#decision-34), item A23):

- The message is not a list, or it is a list with a remainder.
- The list has fewer than two elements.
- The second element is not exactly one string.
- The first element is not a reply name, by the name rule below.

**The name rule.** A value is a reply name when it is exactly one unforgeable name, or exactly one bundle with the write flag (`reified/handle.rs:201-210`).
A typed reply goes only to a reply name, so the process never sends a reply on a forged channel.
A dropped message changes no space, but the COMMs of the forwarder and of `HANDLE_OP` stay in the log with their charges.

*Table 4. Examples of the name rule.*

| Value of `ret` | Reply name |
|---|---|
| `*r`, for a name `r` of `new` | Yes |
| A space name, for example a name from `gensym` | Yes |
| `bundle+{*r}` and `bundle{*r}` | Yes |
| `bundle-{*r}` and `bundle0{*r}` | No |
| `"ret"`, `1`, and `Nil` | No |
| `*r \| 1`, a name beside another term | No |

## 5. Handle methods

Table 5 lists the five methods of a handle (`reified/handle.rs:18-24`).
The arity counts the arguments after `ret` and the method name.

The example `handle-methods` calls `gensym`, `produce`, `qualifier` and `config` with the method-call sugar of the Agents FIPS.
The call `space!m(args)` sends `space!(ret, "m", args…)` with a fresh reply name `ret`.

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

*Table 5. The handle methods.*

| Method | Call | Arity | Named params | Reply | Effect on the space |
|---|---|---|---|---|---|
| `gensym` | `h!(ret, "gensym")` | 0 | None | A new [space name](glossary.md#space-name) $`N(\mathit{sid} \Vert t)`$ | One allocation job: an Array or a Vector space takes and puts its allocator, and every other store stores nothing |
| `qualifier` | `h!(ret, "qualifier")` | 0 | None | The qualifier of the space: `"default"`, `"temp"` or `"seq"` | None |
| `config` | `h!(ret, "config")` | 0 | None | The config map of the space | None |
| `produce` | `h!(ret, "produce", ch, msg)` | 2 | `persistent`: a Boolean, `false` when absent, and the send params of the discipline | `Nil`, after the operation | One produce on the collection of `ch` |
| `consume` | `h!(ret, "consume", pairs, arrow, body)` | 3 | None | `Nil`, after the operation | One consume on the collection of the channel of one pair, or one join over the collections of two or more pairs |

**`gensym`.** The method runs one allocation job with the random state and the cost authority of the call (`reified/handle.rs:383-410`).
An Array or a Vector space gives its next index name $`N(\mathit{sid} \Vert \mathrm{u64\_be}(i))`$ ([specification AL-2](specification.md#24-the-allocator-of-array-and-vector-spaces)).
Every other store answers `Baseline`, and the method gives the random-tail name of the store.

The tail $`t`$ of a random-tail name has 32 bytes, from the random state of the call under the purpose byte `0x02`.
Each call has its own random state, so two calls give two names.
The name routes to the space by itself (rule R3), so a send or a receive on it reaches the space in every context.

**`config`.** The reply is the map that the config record holds, with the six keys of the [specification](specification.md#11-spaces).
The record holds the identity of a theory as bytes, and the reply shows it as the identity map of rule TH-2a ([decision 74](decision-register.md#decision-74)).
For example, `config.get("outer")` gives `"hashmap"`, and `config.get("theory")` gives `Nil` for a space without a theory.

**`produce`.** The method removes `persistent` and passes the other named params to the agent of the space, which checks their values before the bump.
The method runs one collection operation on the space of the handle, as a send does (`reified/handle.rs:441-483`).
The key of `ch` names the collection by rule HM-1 of the specification, also when `ch` is a name of another space.
The record gets the elements of the list `msg` as its payload, and the random state and the cost authority of the call.

The method dispatches the fires of the operation, and it replies `Nil`.
The example `vectordb-queries` stores persistent records with `persistent=true`, so each of its queries sees every record.

The reply runs as the last branch of the operation, as a task of the dispatch scope ([design N6](design.md#n6-scheduling)).
So the reply and the fired bodies are parallel processes, as in the equivalent Rholang program.

**`consume`.** The method runs a receive that the translation of a receive on a user agent describes ([decision 77](decision-register.md#decision-77), items HC-1 to HC-3).
So a wrapper agent can forward the calls of its translations to a native space.
The call is `h!(ret, "consume", pairs, arrow, body)`, where `body` is the body capability.
`pairs` is the list $`[(m_0, c_0), \ldots, (m_{n-1}, c_{n-1})]`$ of the matcher capability and the channel of each bind.
The method makes these checks in this order (`reified/handle.rs:529-662`):

1. The arrow is `"<-"`, `"<="`, or `"<<-"` (`InvalidArrow`).
2. `pairs` is a non-empty list of tuples with two elements, and `body` is a body id of KIND B, bare or under one bundle (`BadArgument`).
3. The state of each matcher decodes strictly from a bare id of KIND X (`ForeignMatcher`, with the index of the first foreign matcher).
4. The states describe one receive (`MatcherSetMismatch`). They have one receive tag, the bind index $`i`$ at pair $`i`$, $`n`$ binds, one total, the offsets in order, and at most one guard.
5. Each channel $`c_i`$ takes the read rule of the baseline receive (`BadArgument` with `"pairs"`).

Then one pair runs one consume on the collection of $`c_0`$, and two or more pairs run one join on the space.
The patterns are the patterns of the states, and the guard is the guard of the one state that has a guard.
The arrow gives the persistence and the peek of the receive.
The body of the receive sends the map $`\{\texttt{"g"}: v_g \mid g < \mathrm{total}\}`$ of the bound values to the body capability, with the random state of the call.
So the body capability gets the map that the matcher process would give the agent.
A matcher state that no translation built cannot reach the space, because the decode accepts exactly the image of the encoder ([decision 77](decision-register.md#decision-77), point C).

The method replies `Nil` as the last branch of the operation, as `produce` does.

The next agent forwards the calls of its translations to a native space with the method-call sugar.
So the native handle runs the `consume` of a receive in the context of the agent.

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

## 6. Replies

A process replies with one datum on the reply name: a list with exactly one element, the reply value.
So the receive `for (@v <- ret)` binds `v` to the reply value.
The reply goes through the R7 backend, which routes `ret` by the rules of [section N5](design.md#n5-routing-and-scope) without a use context.
A reply on a space name therefore reaches the collection of that name in its space.

The bytes of a reply are consensus state.
The reply datum enters the event log and the history like every datum, so every validator must compute the same bytes.
For this reason, the code builds each reply value in canonical form, with the bit sets that the normalizer computes.

## 7. Typed replies

A typed reply is the reply of a call with a protocol error ([decision 34](decision-register.md#decision-34), item A23).
The call does not abort, and the deploy continues.
The reply is one datum, the tuple of three elements that `typed_error` builds (`reified/handle.rs:213-223`):

```text
("rho:space:error", code, detail)
```

- The first element is the string `"rho:space:error"` (`SPACE_ERROR_REPLY_TAG`).
- The second element is the code as a string, one of the twelve codes of Table 6.
- The third element is the detail map. Its keys are strings, and the map has the canonical order of `ParMap`.

A typed reply of `HANDLE_OP` holds the key `"method"`, with the method name of the call as a string.
A typed reply of `getSpaceAgent` has the empty detail map `{}`.

*Table 6. The codes of typed replies and their detail maps (`spaces/errors.rs:312-327`).*

| Code | Process | Trigger | Detail map |
|---|---|---|---|
| `MalformedMessage` | `HANDLE_OP` | The message has a reply name and a method name, but its params value is not canonical. | `{"method": String}` |
| `UnknownMethod` | `HANDLE_OP` | The method name is not `"config"`, `"consume"`, `"gensym"`, `"produce"`, or `"qualifier"`. | `{"method": String, "suggestion": String or Nil}` |
| `BadArgument` | `HANDLE_OP` | An argument has a wrong form. Table 7 gives each value of `"argument"`. | `{"argument": String, "method": String}` |
| `UnknownParam` | `HANDLE_OP` | A named param is outside the schema of the method. The first unknown key in bytewise order counts. | `{"method": String, "param": String, "suggestion": String or Nil}` |
| `InvalidArrow` | `HANDLE_OP` | The arrow of `consume` is not `"<-"`, `"<="`, or `"<<-"`. | `{"method": String}` |
| `ForeignMatcher` | `HANDLE_OP` | A matcher of `consume` is not a matcher id of KIND X, bare or under one bundle, whose state decodes strictly. | `{"index": Int, "method": String}` |
| `MatcherSetMismatch` | `HANDLE_OP` | The matcher states of `consume` do not describe one receive: two receive tags, a bind index out of place, another count of binds, another total, a gap in the offsets, or two guards. | `{"method": String}` |
| `DanglingHandle` | `getSpaceAgent` | The space of the handle has no config record, or it is a temp space of an ended deploy. | `{"spaceId": ByteArray}` |
| `NotANativeSpace` | `getSpaceAgent` | The reference is a value that no Reified id names, so a user agent would implement it. | `{}` |
| `NotASpaceRef` | `getSpaceAgent` | The reference is a Reified id that is not a handle, for example a space name or a factory capability. | `{}` |
| `EmptySpaceRef` | `getSpaceAgent` | The reference is `Nil`. | `{}` |
| `HandleNotWritable` | `getSpaceAgent` | The reference is a handle under a bundle without the write flag. | `{}` |

The values of the detail keys have these meanings:

- `"method"` is the method name exactly as the call gave it, for example `"prodcue"`.
- `"suggestion"` is the suggested method name or param key, or `Nil` when no candidate is at a distance of at most 2.
- `"param"` is the unknown key exactly as the call gave it.
- `"index"` is the position of the first foreign matcher in `pairs`, from 0.

*Table 7. The values of `"argument"` in a `BadArgument` reply (`reified/handle.rs:441-714`).*

| `"argument"` | Method | Trigger |
|---|---|---|
| `"arguments"` | Every method | The call has another number of arguments than the arity of Table 5. |
| `"ch"` | `produce` | `ch` is a bundle without the write flag. |
| `"msg"` | `produce` | `msg` is not a list, or it is a list with a remainder. |
| `"persistent"` | `produce` | The value of the param `persistent` is not a Boolean. |
| `"pairs"` | `consume` | `pairs` is not a non-empty list of tuples with two elements, or a channel of a pair is a bundle without the read flag. |
| `"body"` | `consume` | `body` is not a body id of KIND B, bare or under one bundle. |

*Table 8. Examples of typed replies from the test suites.*

| Call | Reply |
|---|---|
| `h!(ret, "prodcue")` | `("rho:space:error", "UnknownMethod", {"method": "prodcue", "suggestion": "produce"})` |
| `h!(ret, "frobnicate")` | `("rho:space:error", "UnknownMethod", {"method": "frobnicate", "suggestion": Nil})` |
| `h!(ret, "gensym", 1)` | `("rho:space:error", "BadArgument", {"argument": "arguments", "method": "gensym"})` |
| `h!(ret, "produce", "jobs", 7)` | `("rho:space:error", "BadArgument", {"argument": "msg", "method": "produce"})` |
| `h!(ret, "produce", "jobs", [9]; persistant=true)` | `("rho:space:error", "UnknownParam", {"method": "produce", "param": "persistant", "suggestion": "persistent"})` |
| `h!(ret, "produce", "jobs", [9]; priority=1)` | `("rho:space:error", "UnknownParam", {"method": "produce", "param": "priority", "suggestion": Nil})` |
| `getSpaceAgent!(ret, Nil)` | `("rho:space:error", "EmptySpaceRef", {})` |

A typed reply never changes a space: a `produce` call with a typed reply stores no record.

## 8. getSpaceAgent

The call `getSpaceAgent!(ret, s)` gives the factory capability of the family of the space `s` (`reified/system_processes.rs:329-372`).
The process classifies `s` with the five rules of `classify_space_ref` (`reified/scope.rs:148-187`):

1. The empty `Par` gives the typed reply `EmptySpaceRef`.
2. The process removes at most one bundle, as a send does.
3. A handle id, bare or under a bundle with the write flag, names a native space. Under a bundle without the write flag, it gives `HandleNotWritable`.
4. Any other Reified id gives `NotASpaceRef`.
5. Every other value names a user agent and gives `NotANativeSpace`, because the node does not know the constructor of an agent.

For a native space, the process reads the config record in a read-only space job, which writes nothing.
It replies the factory capability of the family that the record names, which is the value that the URN of the family binds ([decision 9.4](decision-register.md#decision-9-4)).
So `getSpaceAgent!(ret, h)` gives a capability that creates new spaces of the family of `h`.
A missing config record gives the typed reply `DanglingHandle`, with the space id in its detail map.
A temp space of an ended deploy gives the same reply before any job ([specification 4.2](specification.md#42-temp), rule QF-9).

Like the factories and `assertEqual`, the process applies no name rule to `ret`.
The next example calls `getSpaceAgent` and `assertEqual`:

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

## 9. assertEqual

The call `assertEqual!(ret, a, b)` has the arity 3 ([decision 33](decision-register.md#decision-33)).
It replies `true` when the sorted forms of `a` and `b` are equal, and `false` in every other case (`reified/system_processes.rs:430-444`).
The sort is the one of `ParSortMatcher`, so the order of parallel processes, of map entries, and of set elements does not count.
The order of list elements and of tuple elements counts.

*Table 9. Examples of `assertEqual` from the test suite.*

| `a` | `b` | Reply |
|---|---|---|
| `1 \| 2` | `2 \| 1` | `true` |
| `[1, 2]` | `[2, 1]` | `false` |
| `{"a": 1, "b": 2}` | `{"b": 2, "a": 1}` | `true` |
| `Set(1, 2)` | `Set(2, 1)` | `true` |
| `(1, 2)` | `(2, 1)` | `false` |

## 10. Aborts

A call aborts the deploy when the error is not a protocol error of the call.
Table 10 lists the aborts that a call can cause, and section [N8 of the design](design.md#n8-errors) lists every abort code.
The last column names the example of `examples/reified_rspaces/` that reaches each code, and section 10.1 shows each example.
For a code that no program on a native chain can reach, the column names the test that reaches it and the reason.

*Table 10. The aborts of calls.*

| Code | Call | Trigger | Example or test |
|---|---|---|---|
| `PersistentFactoryCall` | A factory | A persistent send `!!` | `factory-rejects-a-persistent-call` |
| `PersistentOpCall` | Every other process of Tables 2, 2b and 2c, except `embed` and `ann` | A persistent send `!!` | `system-op-rejects-a-persistent-call` |
| `SystemProcessShapeError` | `embed` and `ann` of Table 2c | A persistent send `!!`, at its fire (base rule P2, [decision 106](decision-register.md#decision-106)) | test `vectordb_embed_and_ann_reject_params_and_persistent_sends` of `rholang/tests/reified_vectordb_chroma.rs`, in a build with the feature `chromadb`: the code is the base error of rule P2 of DR-114, not an abort code of Reified RSpaces |
| `PersistentHandleCall` | A handle | A persistent send `!!` | `handle-rejects-a-persistent-call` |
| `PersistentCapabilityCall` | A matcher or a body capability of a user agent | A persistent send `!!` | `agent-capability-rejects-a-persistent-call` |
| `PersistentSendOnAgentSpace` | Every send on a space that a user agent implements | A persistent send `!!`, or the persistent reply of a system process to an agent-minted name | `agent-space-rejects-a-persistent-send` |
| `ParamsUnsupportedOnAgentSpace` | Every send on a space that a user agent implements | A params section | `agent-space-rejects-params` |
| `ParamsUnsupported` | `getSpaceAgent`, `assertEqual` and the processes of Table 2c | A params section | `system-op-rejects-params` |
| `Malformed` | Every call with params | A params section that is not canonical | The test `reified_hook_errors_are_typed`. The normalizer writes only canonical params, and a program cannot write the field, so only a hand-built term reaches the code. |
| `PositionalParamsUnsupported` | A factory, a handle | A positional param | `factory-rejects-a-positional-param` |
| `FamilyUnavailable` | A factory | Gate 1 of Table 3 | The test `pattern_enforcement_unsupported_pairs_reject`. Every URN with a binding names a family with a schema, so only a hand-built call reaches the code. |
| `BadConstructorArgument` | A factory | Gates 2 to 4 of Table 3 | `factory-rejects-a-bad-argument` |
| `QualifierUnavailable` | A factory, until decision 90 | Retired by [decision 90](decision-register.md#decision-90) (C6, step D) with gate 3 in legacy execution. A factory call outside native execution fails with `RuntimeUnavailable` before every gate. | None. Every deploy runs in native execution. |
| `UnknownTheory` | A factory | Gate 4 of Table 3 | `factory-rejects-an-unknown-theory` |
| `UnknownParam` | A factory, and a send with a param that its collection does not take | Gate 6 of Table 3 | `factory-rejects-an-unknown-param`, `queue-rejects-a-send-param` |
| `MissingParam` | A factory | Gate 7 of Table 3 | `factory-rejects-a-missing-param` |
| `InvalidParam` | A factory, and a send with a param value outside its range | Gates 8 and 9 of Table 3 | `factory-rejects-an-invalid-param`, `priority-queue-rejects-a-level` |
| `TheoryUnavailable` | Every call that reads a space | The config names a theory with no checker in this build ([specification 5](specification.md#5-theories), rule TH-3) | The test `theory_unavailable_fails_each_config_read`. The factory stores only the identities of builtins, so only a hand-built config reaches the code. |
| `TheoryRefuted` | `produce`, and every send | A value of the send that the theory of the space refutes, before any staging (rule TH-5). The message names the theory and the index of the value. | `theory-refuses-a-value` |
| `TheoryUndetermined` | `produce`, and every send | A value of the send that the theory of the space cannot decide (rule TH-5) | The test `theory_undetermined_budget0_aborts`. A builtin always decides, so only a test checker reaches the code. |
| `NestedCommitGuard` | Every call | A transaction with two or more theory leases (rule TH-9) | The test `theory_nested_commit_guard`. The builtins take no lease, so only a test checker reaches the code. |
| `SpaceAlreadyExists` | A factory | A config record on $`C(\mathit{sid})`$ | The test `prop_space_already_exists_is_deterministic_abort`. Each call has a random state of its own, so only a repeated random state reaches the code. |
| `DanglingHandle` | A handle | The space has no config record, or it is a temp space of an ended deploy. The message names the space id. | `temp-space-ends-with-its-deploy` |
| `SeqSent` | Every call, and every send | The data of the send holds a name of a seq space of the evaluation ([specification 4.3](specification.md#43-seq), rule QF-14). The message names the space id. | `space-qualifiers-seq-sent` |
| `SeqConcurrent` | `produce`, and every send and receive | The operation uses a channel of a seq space whose last use is not an ancestor of the operation (rule QF-15). The message names the space id. | `seq-rejects-concurrent-uses` |
| `PayloadNotEmpty` | `produce`, and every send | The message of a send to a collection of a HashSet space is not empty ([specification 2.7](specification.md#27-hashset), rule HS-2) | `hashset-rejects-a-payload` |
| `PatternNotEmpty` | Every receive | A bind on a collection of a HashSet space has a pattern, a binder or a remainder (rule HS-3) | `hashset-rejects-a-pattern` |
| `QuerySpecUnsupportedOnAgentSpace` | Every receive on a space that a user agent implements | A query section | `agent-space-rejects-a-query` |
| `AgentGuardSpansJoin` | Every receive on a space that a user agent implements | A guard that reads the binders of two or more binds | `agent-space-rejects-a-spanning-guard` |
| `AgentPatternTooDeep` | Every receive on a space that a user agent implements | A matcher state that is nested deeper than the decode limit of 100 | `agent-space-rejects-a-deep-pattern` |
| `CrossSpaceJoin` | Every receive | A join whose channels route to two or more spaces, to a space and the default space, or to a user agent and another target | `cross-space-join` |
| `Inconsistent` | Every call that reads a space | A record that is not well formed | The test `space_counter_rejects_corrupt_counters`. A program cannot write a record of a space. |
| `CounterOverflow` | `produce`, and every operation on an ordered collection | A counter at its greatest value | The test `space_counter_rejects_corrupt_counters`. A counter reaches its greatest value only after about $`2^{63}`$ operations on one collection. |

The tests of `FamilyUnavailable` are in `rholang/tests/reified_pattern_enforcement.rs` and `rholang/tests/reified_factory.rs`.
The tests of the theory codes are in `rholang/src/rust/interpreter/spaces/theory/tests.rs`, and the counter test is in `rholang/src/rust/interpreter/spaces/counter.rs`.
The test of `SpaceAlreadyExists` is in `rholang/tests/reified_p4_props.rs`.

### 10.1 The examples of the aborts

Each example ends with the abort of its code, and the harness checks only that code.
The one exception is `factory-rejects-a-forged-params-tuple`, which ends normally and shows that a former marker tuple grants nothing.
The comments of each example explain how it ends.

**Persistent calls.**

```rholang
// A persistent send `!!` to a factory aborts the deploy with
// PersistentFactoryCall. The runtime sends a persistent message again after
// each call, so the call could create spaces without end.
new factory(`rho:space:hashmap:queue`), ret in {
  factory!!(*ret, "default")
}
```

```rholang
// A persistent send `!!` to a system process of Reified RSpaces, here
// assertEqual, aborts the deploy with PersistentOpCall.
new assertEqual(`rho:assert:assertEqual`), ret in {
  assertEqual!!(*ret, 1, 1)
}
```

```rholang
// A persistent send `!!` to the handle of a space aborts the deploy with
// PersistentHandleCall.
new factory(`rho:space:hashmap:queue`), ret in {
  for (queue <- factory!?("default")) {
    queue!!(*ret, "gensym")
  }
}
```

```rholang
// With each consume call, the node gives a user agent one matcher for each
// bind of the receive and a body capability. A persistent send `!!` on the
// body capability aborts the deploy with PersistentCapabilityCall. The same
// rule applies to the matchers.
new agent in {
  for (_, @"consume", @pairs, @arrow, body <= agent) { body!!({}) } |
  use agent { for (@task <- @"jobs") { Nil } }
}
```

```rholang
// In a use block of a user agent, a forged name belongs to the agent, and a
// send on it becomes a produce call of the agent. A persistent send `!!`
// aborts the deploy with PersistentSendOnAgentSpace before the agent gets a
// call.
new agent in {
  use agent { @"jobs"!!("task") }
}
```

**Params.**

```rholang
// A send on a space of a user agent takes no params. A send with a params
// section aborts the deploy with ParamsUnsupportedOnAgentSpace before the
// agent gets a call.
new agent in {
  use agent { @"jobs"!("task"; priority=1) }
}
```

```rholang
// getSpaceAgent and assertEqual take no params. A call with a params section
// aborts the deploy with ParamsUnsupported.
new assertEqual(`rho:assert:assertEqual`), ret in {
  assertEqual!(*ret, 1, 1; strict=true)
}
```

```rholang
// Changed by decision 112 (ruling 9): the params of a send are a field of the send, so a program
// cannot take a params tuple out of its quoted code. The text before:
// ```
// // A program can take a marker of the Reified syntax out of its own quoted
// // code, because a marker is an ordinary name in a term. The marker gives no
// // capability beyond the syntax itself: the node accepts a marker tuple only in
// // the exact form that the normalizer writes. The deploy sends a quoted send
// // with a params section as data, takes the marker of the params section out of
// // it, and builds a params tuple with empty sections, which the normalizer
// // never writes. The factory call with that tuple aborts the deploy with
// // Malformed.
// new factory(`rho:space:hashmap:queue`), ret, code in {
//   code!(@"x"!(1; k=1)) |
//   for (@sent <- code) {
//     match sent {
//       @"x"!(1, (marker, positional, named)) => {
//         factory!(*ret, "default", (marker, [], {}))
//       }
//     }
//   }
// }
// ```
// The params of a send are a field of the send, so a program cannot take a
// params tuple out of its own quoted code. A former marker of the Reified
// syntax is an ordinary name and gives no capability. The deploy sends a quoted
// send with a params section as data and tries to take a params tuple out of
// it. The pattern does not match, because the quoted send holds no params in
// its data, and the wildcard case reports on a result channel.
new factory(`rho:space:hashmap:queue`), ret, code in {
  code!(@"x"!(1; k=1)) |
  for (@sent <- code) {
    match sent {
      @"x"!(1, (marker, positional, named)) => {
        factory!(*ret, "default", (marker, [], {}))
      }
      _ => {
        @"rrs.example.forged-params"!("no marker tuple")
      }
    }
  }
}
```

```rholang
// A factory takes only named params. A positional param aborts the deploy with
// PositionalParamsUnsupported before the factory creates a space.
new factory(`rho:space:hashmap:queue`) in {
  for (queue <- factory!?("default"; 4)) { Nil }
}
```

**The gates of a factory.**

```rholang
// The arguments of a factory are an optional qualifier, then an optional
// theory, then the params. Any other argument aborts the deploy with
// BadConstructorArgument.
new factory(`rho:space:hashmap:queue`) in {
  for (queue <- factory!?(1)) { Nil }
}
```

```rholang
// The params of a factory follow the schema of its family. A queue has no
// params, so a named param aborts the deploy with UnknownParam. The message of
// the error names the param.
new factory(`rho:space:hashmap:queue`) in {
  for (queue <- factory!?("default"; capacity=4)) { Nil }
}
```

```rholang
// An Array space needs the param `size`, the number of its names. A factory
// call without a required param aborts the deploy with MissingParam, and the
// message names the param.
new factory(`rho:space:array:bag`) in {
  for (array <- factory!?("default")) { Nil }
}
```

```rholang
// The param `size` of an Array space is an Int of at least 1. A factory call
// with a value outside the range of its param aborts the deploy with
// InvalidParam, and the message names the param.
new factory(`rho:space:array:bag`) in {
  for (array <- factory!?("default"; size=0)) { Nil }
}
```

**Theories, qualifiers and HashSet spaces.** The first two files are two deploys of one example.

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

```rholang
// The second deploy of the temp example. It takes the handle that the first
// deploy published and calls it. The temp space ended with the first deploy,
// so the call aborts the deploy with DanglingHandle.
for (scratch <- @"rrs.example.temp.handle") {
  for (@qualifier <- scratch!?("qualifier")) { Nil }
}
```

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

```rholang
// A HashSet space holds units. A send with data aborts the deploy with
// PayloadNotEmpty.
new factory(`rho:space:hashset:bag`) in {
  for (space <- factory!?("seq")) {
    use space {
      @"seen"!(1)
    }
  }
}
```

```rholang
// A HashSet space holds units, so a receive binds nothing. A receive with a
// binder aborts the deploy with PatternNotEmpty.
new factory(`rho:space:hashset:bag`) in {
  for (space <- factory!?("seq")) {
    use space {
      for (@value <- @"seen") { Nil }
    }
  }
}
```

**User agents and joins.**

```rholang
// A receive on a space of a user agent takes no query. A receive with a query
// section aborts the deploy with QuerySpecUnsupportedOnAgentSpace before the
// agent gets a call.
new agent in {
  use agent { for (@x <- @"jobs" ~ 5) { Nil } }
}
```

```rholang
// The node gives each bind of a receive on a user agent its own matcher, and
// a guard belongs to one matcher. A guard that reads the binders of two binds
// aborts the deploy with AgentGuardSpansJoin before the agent gets a call.
new agent in {
  use agent { for (@x <- @"left" & @y <- @"right" where x == y) { Nil } }
}
```

```rholang
// The node puts the pattern of each bind of a receive on a user agent in the
// id of the matcher of the bind, and a node decodes such an id only up to a
// fixed depth. A pattern of 30 nested lists is too deep, so the receive
// aborts the deploy with AgentPatternTooDeep before the agent gets a call. A
// pattern of 29 nested lists is the deepest that the node accepts.
new agent in {
  use agent {
    for (@[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[x]]]]]]]]]]]]]]]]]]]]]]]]]]]]]] <- @"jobs") { Nil }
  }
}
```

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

## 11. Limits

- A reply on a forged reply name goes to the default space (rule R7). So a reply name should come from `new`.
- The method `consume` takes only the matchers that the node builds for a receive. So a program calls it only through a user agent ([section 5](#5-handle-methods)).
- `getSpaceAgent` on a user agent replies `NotANativeSpace`, because the node does not know the constructor of an agent.
- A factory takes only the builtin theories. The installed theories of F1r3lang come with phase P12, after the F1r3lang merge ([F1r3lang integration](f1r3lang-integration.md)).
- `rho:lang:vector` has five operations. The other operations of the legacy branch come with TensorLogic in phase P12 ([decision 88](decision-register.md#decision-88)).
- Spaces run only in native execution, as every deploy does. Outside it, every factory call fails with `RuntimeUnavailable` before every gate ([decision 90](decision-register.md#decision-90)).
- The factory op `0x20` is reserved for native engines such as MeTTaTron, and it has no URN ([decision 2](decision-register.md#decision-2)).

## 12. User agent-backed spaces

A program can use an agent that it writes as a space, as the FIPS allows ([decision 77](decision-register.md#decision-77)).
The program names the agent in a `use` block or in an annotation `new x : a`, as it names a native space.
The node translates each operation on the space into a call on the agent, and the agent serves the call with its own Rholang code.
Figure 2 shows the translation of a `new`, a send and a receive.
[Chapter 21](../../rholang/21-reified-spaces.md#writing-your-own-space) explains how to write an agent, with the reference agent and the conformance kit.

### 12.1 The calls

*Table 11. The calls of the translation on the agent `A` (design v2 TS-1, TR-7 and TN-4).*

| Operation of the program | Call on `A` | Other effects of the translation |
|---|---|---|
| `new x` in the context of `A`, or `new x : A` | `A!(r, "gensym")` for each agent binder, with a private reply name `r` | The node draws the names of HEAD first. A resume receive on `r` binds `x` to the [agent-minted name](glossary.md#agent-minted-name) of the reply. |
| `c!(d…)` on an agent-minted name of `A`, or on a forged channel `c` in the context of `A` | `A!(sink, "produce", raw, [d…])` | None. |
| `for (p_1 <- c_1 & … & p_n <- c_n) P`, with every `c_i` on `A` | `A!(sink, "consume", [(m_1, raw_1), …], arrow, b)` | One persistent matcher receive for each bind, and the body receive. The body receive is linear for `"<-"` and `"<<-"` and persistent for `"<="`. |

The value `A` of a call is the call channel: the agent reference without one bundle with the write flag.
The raw channel `raw` is the raw part of an agent-minted name, or the forged channel itself.
The return channel `sink` is the write bundle of the sink process of Table 2b, which absorbs any reply.
Each `m_i` is the write bundle of the matcher capability of bind `i`, and `b` is the write bundle of the body capability.
Table 10 lists the aborts of these operations, from `PersistentSendOnAgentSpace` to `CrossSpaceJoin`.

The example `agent-wrapper-forwards-to-native` of section 5 runs each row of Table 11: its `new`, its send and its receive.
The reference agent `examples/reified_rspaces/agent_hashmap_bag.rho` runs them too.

### 12.2 The protocol

The agent replies to each `gensym` call with one fresh name on its reply name.
It stores the message of a `produce` call, or it fires a stored consume with it.
For a `consume` call, it tests stored messages with the matcher of each bind through the call `m!(ret, proc)` of section 2.
When every bind has a match, the agent sends the union of the maps `env` to the body capability with `b!(env)`.
The arrow `"<-"` removes the chosen messages, `"<<-"` keeps them, and `"<="` keeps the consume.

### 12.3 The obligations of the agent

The node does not check the behavior of an agent.
The author of an agent keeps three obligations (design v2 section 10):

- **ASO-1.** After each method completes, no stored consume and stored messages exist such that every matcher of the consume accepts its message.
- **ASO-2.** For the arrows `"<-"` and `"<<-"`, the agent sends to the body capability at most once.
- **ASO-3.** The agent replies to each `gensym` call exactly once, with a fresh name.

An agent that breaks an obligation breaks the semantics of its space, but never consensus.
Each call and each send of the agent is an ordinary event of the default space.
So every node charges it and replays it in the same way.
The node also bounds two effects of a faulty agent.
The body receive of a linear receive takes one send, so a second send parks and the body runs at most once.
A second reply to a `gensym` call parks on the reply name, because the resume receive takes one reply.

### 12.4 Trust and cost

The agent sends the values of a receive to the body capability.
So a program trusts the agent to send the values that the matchers gave.
A native handle accepts only the matcher capabilities of a translation ([section 5](#5-handle-methods)), but an agent calls the matchers that a consume brings.
So an agent that waits for a matcher reply while it holds its state stops when a matcher never answers.
Each operation on an agent space costs ordinary COMMs, and each matcher call costs three of them: the call, the forwarder hop and the reply.
The call `getSpaceAgent` on an agent space replies `NotANativeSpace` ([section 8](#8-getspaceagent)).

![Translation of a use block of a user agent A as a sequence. The deploy runs use A { new x in { x!(1) | for (@v <- x) { P } } }. For the new, the reducer draws the names of HEAD, substitutes the body and wraps it in USE(A), and computes the reply name r from base.split_byte(5).split_short(0). It produces A!(r, "gensym") and the resume receive on r. The agent replies a fresh name raw on r, and the resume receive binds x to bundle{(AGENT_TAG, A, raw)}. For the send x!(1), the reducer routes x by R4 to the agent A with raw and produces A!(sink, "produce", raw, [1]); the agent stores (raw, [1]) and offers it to its stored consumes, of which there are none. For the receive for (@v <- x) { P }, the reducer routes x to A, builds the matcher state with the pattern [@v] and no guard, puts a persistent receive on X(m) that forwards to MATCHER_OP and a receive on B(b) that is linear for the arrow <- and holds the body USE(A, P), and produces A!(sink, "consume", [(bundle+{m}, raw)], "<-", bundle+{b}). The agent calls m!(ret, [1]) for the stored message. The forwarder of m calls MATCHER_OP, which decodes the state of m, matches [1] against [@v] and checks the guard, and replies (true, {"0": 1}) on ret. The agent removes the message for the arrow <- and sends b!({"0": 1}), and P runs in the context of A with v = 1.](diagrams/04-user-agent-translation.svg)

*Figure 2. Source: [diagrams/04-user-agent-translation.puml](diagrams/04-user-agent-translation.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

## References

<a id="ref-lev66"></a>[Lev66] V. I. Levenshtein, "Binary codes capable of correcting deletions, insertions, and reversals," *Soviet Physics Doklady*, vol. 10, no. 8, pp. 707–710, 1966.
