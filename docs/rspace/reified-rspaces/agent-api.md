# Reified RSpaces agent API

This document states the interface between a Rholang program and the native spaces of Reified RSpaces.
It gives each call that a program can make, the reply of the call, and each error with its exact form.
Phase P4 writes the sections of the native space family `hashmap:queue`.
Phase P8 adds the sections of [user agent-backed spaces](glossary.md#user-agent-backed-space).

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
- [11. Limits of P4](#11-limits-of-p4)
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

![Lifecycle of a native handle as a sequence. A deploy sends the factory call hmq!(*ret) with its constructor. The reducer routes the op channel [0xE0, 0x01] to the default space, checks the persistent flag, packs the constructor and produces on the op channel. The COMM with the installed factory runs FactoryProcess::call: it validates the constructor and derives sid from the random state of the call. A space intent with the footprint [2] ‖ sid and H(sid) runs one creation transaction: begin on C(sid), S(sid) and H(sid), a check for SpaceAlreadyExists, the config record stored persistently on C(sid), the space counter S(sid) = 0, and the handle continuation parked persistently on H(sid); the commit logs the events. The factory replies bundle+{H(sid)} on ret. Later the deploy calls h!(*ack, "produce", "jobs", [1]): the reducer removes the write bundle, routes H(sid) to the default space and produces on it; the COMM with the persistent handle continuation runs its body, which produces (this, args) on HANDLE_OP. HANDLE_OP checks the bare handle id, reads C(sid) in a read-only space intent that commits nothing, parses the message, and runs the method produce as a collection operation in its own space intent: the counter bump and the head-only offer. It dispatches the fires and replies Nil on ack. A protocol error replies a typed error ("rho:space:error", code, detail) instead, and a missing config record aborts with DanglingHandle.](diagrams/03-native-handle-lifecycle.svg)

*Figure 1. Source: [diagrams/03-native-handle-lifecycle.puml](diagrams/03-native-handle-lifecycle.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

## 2. System processes

P4 installs four system processes (`reified/system_processes.rs:74-125`).
Each one listens on an [op channel](glossary.md#op-channel) $`[\mathtt{0xE0}, \mathit{op}]`$ and has the body reference $`200 + \mathit{op}`$.
The URN map binds each URN to the write bundle of its op channel. So a program can call each process, but it cannot receive on its channel.

*Table 2. The system processes of P4 (`reified/system_channels.rs:49-82`).*

| URN | Op channel | Arity | Body reference | Call | Reply |
|---|---|---|---|---|---|
| `rho:space:hashmap:queue` | `[0xE0, 0x01]` | 2 | 201 | `hmq!(ret, ctor…)` | The handle capability of a new space |
| `rho:space:getSpaceAgent` | `[0xE0, 0x21]` | 2 | 233 | `getSpaceAgent!(ret, s)` | The factory capability of the family of `s`, or a typed reply |
| `rho:assert:assertEqual` | `[0xE0, 0x22]` | 3 | 234 | `assertEqual!(ret, a, b)` | A Boolean |
| `rho:reified:internal:handleOp` | `[0xE0, 0x30]` | 2 | 248 | `(this, message)`, from the handle continuation of a space | The reply of the method, or a typed reply |

*Table 2a. The factories of P5 (`reified/system_channels.rs:338-364`).*

| URN | Op channel | Arity | Body reference | Constructor params |
|---|---|---|---|---|
| `rho:space:hashmap:bag` | `[0xE0, 0x00]` | 2 | 200 | none |
| `rho:space:hashmap:stack` | `[0xE0, 0x02]` | 2 | 202 | none |
| `rho:space:hashmap:set` | `[0xE0, 0x03]` | 2 | 203 | none |
| `rho:space:hashmap:cell` | `[0xE0, 0x04]` | 2 | 204 | none |
| `rho:space:hashmap:priorityqueue` | `[0xE0, 0x05]` | 2 | 205 | `priorities` |
| `rho:space:pathmap:<inner>` | `[0xE0, 0x07]` to `[0xE0, 0x0C]` | 2 | 207 to 212 | `priorities` for a priority queue |
| `rho:space:array:<inner>` | `[0xE0, 0x0D]` to `[0xE0, 0x12]` | 2 | 213 to 218 | `size` (required), `cyclic`, and `priorities` for a priority queue |
| `rho:space:vector:<inner>` | `[0xE0, 0x13]` to `[0xE0, 0x18]` | 2 | 219 to 224 | `priorities` for a priority queue |
| `rho:space:HashMapBagSpace` | `[0xE0, 0x00]` | 2 | 200 | none: the alias of `hashmap:bag` |
| `rho:space:PathMapSpace` | `[0xE0, 0x07]` | 2 | 207 | none: the alias of `pathmap:bag` |

A row of Table 2a is active only when its family is registered, which is exactly when the family dispatch has its arm ([specification FA-1](specification.md#15-families)).
At this commit every row of Table 2a is active. The PathMap families join the table in wave W4 of P5.

Three rules hold for every process of Table 2 and Table 2a:

- **Fixed arity.** A process has a fixed arity ([decision I-18](decision-register.md#i-18)). A call with another arity never reaches the process, and its datum stays on the op channel.
- **No persistent call.** A persistent send `!!` to an op channel aborts the deploy before the call ([decision 32](decision-register.md#decision-32), item A21). The code is `PersistentFactoryCall` for a factory and `PersistentOpCall` for the other processes.
- **No params.** Only a factory and a handle take params. A send with params to `getSpaceAgent` or `assertEqual` aborts with `ParamsUnsupported`.

The URNs of the other 31 families and the two FIPS aliases have no binding in P4.
A program that names one of them fails with the baseline error `No value set for <urn>`, before any space operation.

## 3. Factories

### 3.1 The call

A factory call is a send on the factory capability: `hmq!(ret, a1, …, an)`.
The send hook packs the arguments after `ret` into one list, so the factory receives `[ret, [a1, …, an]]` (`reified/reducer_hooks.rs:987-1027`).
So every factory has the fixed arity 2, and a constructor can have any number of elements.
The params section of the call stays in the list as its last element.

The constructor must be the canonical sequence $`[q?]\,[\mathrm{FREE}?]\,[\mathrm{PARAMS}?]`$ (`models/src/rust/space_factory_urn.rs:366-398`):

- $`q`$ is a string that names a qualifier: `"default"`, `"temp"`, or `"seq"`.
- $`\mathrm{FREE}`$ is a theory reference `free Th(args)`.
- $`\mathrm{PARAMS}`$ is the params section of the call, with named params only.

### 3.2 The gates

The factory checks the constructor with seven gates, in the order of Table 3 (`spaces/factory/constructor.rs:68-122`).
The first gate that fails aborts the deploy with its code, and the later gates do not run.

*Table 3. The gates of a factory call.*

| # | Check | Code of the abort |
|---|---|---|
| 1 | The family has a schema in this phase. | `FamilyUnavailable` |
| 2 | The constructor is the canonical sequence. An unknown qualifier string, a malformed theory or params tuple, and any other element fail here. | `BadConstructorArgument` |
| 3 | The outer store takes the qualifier. The HashSet store takes only `"seq"`. | `BadConstructorArgument` |
| 4 | The qualifier is `"default"`. The qualifiers `"temp"` and `"seq"` arrive in P7. | `QualifierUnavailable` |
| 5 | The constructor names no theory. Theories arrive in P7. | `TheoryUnavailable` |
| 6 | The params are named only. The send hook already checks this rule. | `PositionalParamsUnsupported` |
| 7 | Each named param is in the schema of the family, in the bytewise order of the keys. The schema of `hashmap:queue` is empty. | `UnknownParam` |

The code `UnknownParam` of a factory has a suggestion in its message, but a suggestion needs a schema key, so `hashmap:queue` never gives one.
For example, the constructor `("default", "seq", "temp")` fails at gate 2, and the next program fails at gate 5 with `TheoryUnavailable`:

```rholang
new hmq(`rho:space:hashmap:queue`) in { for (space <- hmq!?(free Nat())) { Nil } }
```

### 3.3 The creation and the reply

After the gates, the factory derives the space id from the random state $`r`$ of the call ([decision I-19](decision-register.md#i-19)):

```math
\mathit{sid} = \mathrm{next}\bigl(\mathrm{split}_{\mathtt{0x01}}(\mathrm{split}_{\mathtt{0xE0}}(r))\bigr)
```

One [space intent](glossary.md#space-intent) then creates the space in one transaction.
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
Its body `[0xE0, 0x30]!(this, args)` forwards the call to `HANDLE_OP` (`reified/factory.rs:127-151`).
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

`HANDLE_OP` processes a call `(this, message)` in this order (`reified/handle.rs:220-368`):

1. When `this` is not a bare handle id, the process drops the call.
2. The process reads the config record in a read-only space intent, which commits nothing. A missing record aborts with `DanglingHandle`, also when the message is malformed.
3. The process applies the [drop rule](#44-the-drop-rule-and-the-name-rule) to the message. A dropped message gets no reply.
4. A trailing params tuple that is not canonical gives the typed reply `MalformedMessage`. The send hook decodes the params of a program call first, and it aborts with `Malformed` on such a tuple.
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

**The name rule.** A value is a reply name when it is exactly one unforgeable name, or exactly one bundle with the write flag (`reified/handle.rs:181-190`).
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

Table 5 lists the five methods of a handle (`reified/handle.rs:16-22`).
The arity counts the arguments after `ret` and the method name.

*Table 5. The handle methods.*

| Method | Call | Arity | Named params | Reply | Effect on the space |
|---|---|---|---|---|---|
| `gensym` | `h!(ret, "gensym")` | 0 | None | A new [space name](glossary.md#space-name) $`N(\mathit{sid} \Vert t)`$ | One allocation job: an Array or a Vector space takes and puts its allocator, and every other store stores nothing |
| `qualifier` | `h!(ret, "qualifier")` | 0 | None | The qualifier of the space, `"default"` in P4 | None |
| `config` | `h!(ret, "config")` | 0 | None | The config map of the space | None |
| `produce` | `h!(ret, "produce", ch, msg)` | 2 | `persistent`: a Boolean, `false` when absent, and the send params of the discipline | `Nil`, after the operation | One produce on the collection of `ch` |
| `consume` | `h!(ret, "consume", pairs, arrow, body)` | 3 | None | None in P4: a typed reply or an abort | None |

**`gensym`.** The method runs one allocation job with the random state and the cost authority of the call (`reified/handle.rs:341-368`).
An Array or a Vector space gives its next index name $`N(\mathit{sid} \Vert \mathrm{u64\_be}(i))`$ ([specification AL-2](specification.md#24-the-allocator-of-array-and-vector-spaces)).
Every other store answers `Baseline`, and the method gives the random-tail name of the store.

The tail $`t`$ of a random-tail name has 32 bytes, from the random state of the call under the purpose byte `0x02`.
Each call has its own random state, so two calls give two names.
The name routes to the space by itself (rule R3), so a send or a receive on it reaches the space in every context.

**`config`.** The reply is the map that the config record holds, with the six keys of the [specification](specification.md#11-spaces).
For example, `config.get("outer")` gives `"hashmap"`, and `config.get("theory")` gives `Nil`.

**`produce`.** The method removes `persistent` and passes the other named params to the agent of the space, which checks their values before the bump.
The method runs one collection operation on the space of the handle, as a send does (`reified/handle.rs:399-441`).
The key of `ch` names the collection by rule HM-1 of the specification, also when `ch` is a name of another space.
The record gets the elements of the list `msg` as its payload, and the random state and the cost authority of the call.

The method dispatches the fires of the operation, and it replies `Nil`.
For example, this program stores a persistent record with the payload `1` on the collection of `"jobs"`:

```rholang
new hmq(`rho:space:hashmap:queue`) in { for (space <- hmq!?("default")) { for (_ <- space!?("produce", "jobs", [1]; persistent=true)) { Nil } } }
```

The reply runs as the last branch of the operation, in a reduction participant of its own.
A dispatch ends the participant that makes it, so the reply cannot follow the fires in that participant ([design N6](design.md#n6-scheduling)).
So the reply and the fired bodies are parallel processes, as in the equivalent Rholang program.

**`consume`.** The method accepts only the matchers of user agents, which P8 adds ([decision 34](decision-register.md#decision-34), item A9).
It checks the arrow, the pairs, the body, and the matchers, in this order (`reified/handle.rs:445-484`).
A call that passes every check aborts with `AgentSpacesUnavailable` until P8.

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
The reply is one datum, the tuple of three elements that `typed_error` builds (`reified/handle.rs:193-203`):

```text
("rho:space:error", code, detail)
```

- The first element is the string `"rho:space:error"` (`SPACE_ERROR_REPLY_TAG`).
- The second element is the code as a string, one of the eleven codes of Table 6.
- The third element is the detail map. Its keys are strings, and the map has the canonical order of `ParMap`.

A typed reply of `HANDLE_OP` holds the key `"method"`, with the method name of the call as a string.
A typed reply of `getSpaceAgent` has the empty detail map `{}`.

*Table 6. The codes of typed replies and their detail maps (`spaces/errors.rs:251-263`).*

| Code | Process | Trigger | Detail map |
|---|---|---|---|
| `MalformedMessage` | `HANDLE_OP` | The message has a reply name and a method name, but its trailing params tuple is not canonical. | `{"method": String}` |
| `UnknownMethod` | `HANDLE_OP` | The method name is not `"config"`, `"consume"`, `"gensym"`, `"produce"`, or `"qualifier"`. | `{"method": String, "suggestion": String or Nil}` |
| `BadArgument` | `HANDLE_OP` | An argument has a wrong form. Table 7 gives each value of `"argument"`. | `{"argument": String, "method": String}` |
| `UnknownParam` | `HANDLE_OP` | A named param is outside the schema of the method. The first unknown key in bytewise order counts. | `{"method": String, "param": String, "suggestion": String or Nil}` |
| `InvalidArrow` | `HANDLE_OP` | The arrow of `consume` is not `"<-"`, `"<="`, or `"<<-"`. | `{"method": String}` |
| `ForeignMatcher` | `HANDLE_OP` | A matcher of `consume` is not a matcher id of KIND X, bare or under one bundle. | `{"index": Int, "method": String}` |
| `DanglingHandle` | `getSpaceAgent` | The space of the handle has no config record. | `{}` |
| `NotANativeSpace` | `getSpaceAgent` | The reference is a value that no Reified id names, so a user agent would implement it. | `{}` |
| `NotASpaceRef` | `getSpaceAgent` | The reference is a Reified id that is not a handle, for example a space name or a factory capability. | `{}` |
| `EmptySpaceRef` | `getSpaceAgent` | The reference is `Nil`. | `{}` |
| `HandleNotWritable` | `getSpaceAgent` | The reference is a handle under a bundle without the write flag. | `{}` |

The values of the detail keys have these meanings:

- `"method"` is the method name exactly as the call gave it, for example `"prodcue"`.
- `"suggestion"` is the suggested method name or param key, or `Nil` when no candidate is at a distance of at most 2.
- `"param"` is the unknown key exactly as the call gave it.
- `"index"` is the position of the first foreign matcher in `pairs`, from 0.

*Table 7. The values of `"argument"` in a `BadArgument` reply (`reified/handle.rs:399-484`).*

| `"argument"` | Method | Trigger |
|---|---|---|
| `"arguments"` | Every method | The call has another number of arguments than the arity of Table 5. |
| `"ch"` | `produce` | `ch` is a bundle without the write flag. |
| `"msg"` | `produce` | `msg` is not a list, or it is a list with a remainder. |
| `"persistent"` | `produce` | The value of the param `persistent` is not a Boolean. |
| `"pairs"` | `consume` | `pairs` is not a non-empty list of tuples with two elements. |
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

The call `getSpaceAgent!(ret, s)` gives the factory capability of the family of the space `s` (`reified/system_processes.rs:187-223`).
The process classifies `s` with the five rules of `classify_space_ref` (`reified/scope.rs:106-129`):

1. The empty `Par` gives the typed reply `EmptySpaceRef`.
2. The process removes at most one bundle, as a send does.
3. A handle id, bare or under a bundle with the write flag, names a native space. Under a bundle without the write flag, it gives `HandleNotWritable`.
4. Any other Reified id gives `NotASpaceRef`.
5. Every other value names a user agent and gives `NotANativeSpace` in P4.

For a native space, the process reads the config record in a read-only space intent, which writes nothing.
It replies the factory capability of the family that the record names, which is the value that the URN of the family binds ([decision 9.4](decision-register.md#decision-9-4)).
So `getSpaceAgent!(ret, h)` gives a capability that creates new spaces of the family of `h`.
A missing config record gives the typed reply `DanglingHandle`.

Like the factories and `assertEqual`, the process applies no name rule to `ret`.

## 9. assertEqual

The call `assertEqual!(ret, a, b)` has the arity 3 ([decision 33](decision-register.md#decision-33)).
It replies `true` when the sorted forms of `a` and `b` are equal, and `false` in every other case (`reified/system_processes.rs:262-276`).
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

*Table 10. The aborts of calls.*

| Code | Call | Trigger |
|---|---|---|
| `PersistentFactoryCall`, `PersistentOpCall`, `PersistentHandleCall` | Every call | A persistent send `!!` |
| `ParamsUnsupported` | `getSpaceAgent`, `assertEqual` | A params section |
| `Malformed` | Every call with params | A params section that is not canonical after the refresh of [I-20](decision-register.md#i-20) |
| `PositionalParamsUnsupported` | A factory, a handle | A positional param |
| `FamilyUnavailable`, `BadConstructorArgument`, `QualifierUnavailable`, `TheoryUnavailable`, `UnknownParam` | A factory | A gate of Table 3 |
| `SpaceAlreadyExists` | A factory | A config record on $`C(\mathit{sid})`$ |
| `DanglingHandle` | A handle | The space has no config record |
| `AgentSpacesUnavailable` | `consume` | Matchers of user agents, until P8 |
| `Inconsistent` | Every call that reads a space | A record that is not well formed |
| `CounterOverflow` | `produce` | A counter at its greatest value |

## 11. Limits of P4

- Only the family `hashmap:queue` has a factory, and P5 and P6 add the other families.
- The method `consume` has no effect until P8, which adds the matchers of user agents.
- `getSpaceAgent` has no alias in P4. P5 adds the alias ([decision 34](decision-register.md#decision-34), item A15).
- A reply on a forged reply name goes to the default space, so a reply name should come from `new`.

## References

<a id="ref-lev66"></a>[Lev66] V. I. Levenshtein, "Binary codes capable of correcting deletions, insertions, and reversals," *Soviet Physics Doklady*, vol. 10, no. 8, pp. 707–710, 1966.
