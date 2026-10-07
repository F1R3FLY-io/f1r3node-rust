# PathMap prefix delivery

This document is the reference of the PathMap spaces of phase P5 (P5e).
It describes the store, the path markers, the delivery plans, the order of an operation, the merge, and the formal models.

In a PathMap space, a channel names a path, and a send on a path also reaches every prefix of the path ([FIPS-RRS](#ref-fips-rrs)).
Wave W2 added the pure layer of paths, keys and re-rooting ([specification 2.5](specification.md#25-pathmap-paths-and-re-rooting), [design N11.7](design.md#n117-the-pure-pathmap-layer)).
Wave W3 adds the store and the delivery on top of that layer.

**Status.** Phase 1 of wave W3 is complete: the store, the markers, the delivery planner, and the formal models.
Phase 2 connects the planner to the join engine through a prefix source, and it adds the dispatch and the integration tests ([section 12](#12-status-and-open-items)).

## Contents

1. [Terms and notation](#1-terms-and-notation)
2. [The model](#2-the-model)
3. [The store](#3-the-store)
4. [Path markers and the vehicle](#4-path-markers-and-the-vehicle)
5. [The delivery plans](#5-the-delivery-plans)
6. [The token and the order of an operation](#6-the-token-and-the-order-of-an-operation)
7. [Merges and prefix contact](#7-merges-and-prefix-contact)
8. [Metering](#8-metering)
9. [Failures](#9-failures)
10. [Security and determinism](#10-security-and-determinism)
11. [Verification](#11-verification)
12. [Status and open items](#12-status-and-open-items)

The [references](#references) follow the last section.

## 1. Terms and notation

The table defines the symbols of this document.
The [glossary](glossary.md) defines the general terms, for example [datum](glossary.md#datum), [waiter](glossary.md#waiter), [COMM event](glossary.md#comm-event), [peek](glossary.md#peek) and [persistent](glossary.md#persistent).

| Symbol or term | Meaning |
|---|---|
| $`p = [e_1, \ldots, e_n]`$ | A [path](glossary.md#path): a list of path elements. The depth of $`p`$ is $`\lvert p \rvert = n`$. |
| $`\varepsilon`$ | The empty path. It is a prefix of every path. |
| $`q \preceq p`$ | The path $`q`$ is a prefix of $`p`$: $`p = q \mathbin{+\!\!+} s`$ for a path $`s`$. Every path is a prefix of itself. |
| $`p \setminus q`$ | The suffix $`s`$ of $`p`$ after its prefix $`q`$. |
| $`\mathrm{path\_key}(p)`$ and $`\mathrm{prefix\_key}(q)`$ | The byte keys of rule PP-2 ([specification 2.5](specification.md#25-pathmap-paths-and-re-rooting)). |
| $`\mathit{sid}`$ | The [space id](glossary.md#space-id) of the space. |
| $`D(p)`$ | The data channel $`D(\mathit{sid}, \mathrm{path\_key}(p))`$. It holds the datum records of the path $`p`$. |
| $`I(q)`$ | The index channel $`I(\mathit{sid}, \mathrm{prefix\_key}(q))`$. It holds the path markers under $`q`$ and the waiters on $`q`$. |
| $`S(\mathit{sid})`$ | The token of the space: its [space counter](glossary.md#space-counter). |
| $`\mathrm{src}(d)`$ | The source hash of the datum $`d`$: the hash of its produce on $`D(p)`$. It is the identity of $`d`$. |
| $`m_q(d)`$ | The [path marker](glossary.md#path-marker) of the datum $`d`$ on $`I(q)`$. |
| $`v_q(d)`$ | The vehicle of $`d`$ on $`I(q)`$: the incoming record of a delivery to a waiter on $`q`$. No store keeps it. |
| $`R(s, v)`$ | The [re-rooting](glossary.md#re-rooting) of an argument $`v`$ under the suffix $`s`$ (rule PP-5). |
| `K_retire` | A COMM on one channel that removes one record of a datum after its delivery (F4 of [decision 39](decision-register.md#decision-39)). |
| `FIRE_ROUND_CAP` | The bound of 64 fires on one [space transaction](glossary.md#space-transaction) ([decision 49](decision-register.md#decision-49)). |
| MI | The marker invariant of [section 4.3](#43-the-marker-invariant). |

## 2. The model

### 2.1 A send reaches every prefix

A space of the campaign is a tuple space as in Linda ([Gel85](#ref-gel85)).
Its channels are the names of the rho calculus ([MR05](#ref-mr05)).
In a PathMap space, a channel names a path by rule PP-1.
A plain list names the path of its elements, and every other channel `c` names the path `[c]`.

A send on the path $`p`$ is a send on each prefix $`q \preceq p`$ ([decision 9.3](decision-register.md#decision-9-3)).
So a receive on $`q`$ can take any datum whose path extends $`q`$.
The receive gets each argument of the datum re-rooted under the suffix $`p \setminus q`$.
For example, `@[0,1,2]!(42)` reaches a receive on `@[0,1]` as `[2, 42]`, and a receive on `@[0]` as `[1, 2, 42]`.
A send of the PathMap `{|"hi"|}` on `@[0,1,2]` reaches a receive on `@[0,1]` as `{|[2, "hi"]|}`.

The prefixes of a path of depth $`n`$ are the $`n + 1`$ paths

```math
\mathrm{prefixes}(p) = [\, \varepsilon,\; [e_1],\; [e_1, e_2],\; \ldots,\; [e_1, \ldots, e_n] \,]
```

in depth order, and a send meets them in the reverse order, the most specific prefix first.

### 2.2 Why the store keeps path markers

A receive on $`q`$ must find every datum under $`q`$, and the host store cannot list its channels by prefix.
So each prefix $`q`$ has an index channel $`I(q)`$.
A stored datum puts one path marker on the index channel of each prefix of its path.
A receive on $`q`$ then reads one channel, $`I(q)`$, and finds every datum under $`q`$.

The markers also make merges safe.
A new datum on $`p`$ in one branch and a new waiter on $`q \preceq p`$ in another branch both touch $`I(q)`$.
So merge check #2 of the host sees their contact ([section 7](#7-merges-and-prefix-contact)).

Each delivery stays on one channel (F4).
A delivery is a COMM on $`I(q)`$.
The removal of the other records of the datum is a sequence of `K_retire` COMMs, one channel each.
So the engine never makes a multi-channel join ([I-16](decision-register.md#i-16)), which would make merge check #3 flag every later produce on its channels.

### 2.3 An example state

The figure below shows a space after `@[0,1]!(a)` and `@[0,1,2]!(b)`, with a waiter parked on `@[1]`.
The datum $`d_1`$ holds the value `a`, and $`d_2`$ holds `b`.
Each marker shows its datum and its suffix.

```text
  space sid                    the records of each channel
  ───────────────────────────  ─────────────────────────────────────────────
  D(sid, path_key([0,1]))      d₁ = a
  D(sid, path_key([0,1,2]))    d₂ = b
  I(sid, prefix_key(ε))        m(d₁, suffix [0,1])     m(d₂, suffix [0,1,2])
  I(sid, prefix_key([0]))      m(d₁, suffix [1])       m(d₂, suffix [1,2])
  I(sid, prefix_key([0,1]))    m(d₁, suffix [])        m(d₂, suffix [2])
  I(sid, prefix_key([0,1,2]))  m(d₂, suffix [])
  I(sid, prefix_key([1]))      waiter w                (no marker: no datum under [1])
  S(sid)                       the token
```

The datum $`d_1`$ has three markers, one for each prefix of `[0,1]`.
The datum $`d_2`$ has four markers.
A receive on `@[0]` reads $`I([0])`$ and finds both datums, with the suffixes `[1]` and `[1,2]`.

### 2.4 The delivery in one figure

Figure 1 shows the plans of a send, a receive, and the second send of a peek.
[Section 5](#5-the-delivery-plans) states each plan in literate pseudocode.

![Activity diagram of the PathMap delivery. Every operation first checks its path: a path with more than 64 elements fails with PathTooDeep, and an element that holds a PathMap or a zipper fails with UnsupportedPathElement, before any staging. A send of x on p walks PRODUCE-WAITERS(p), the waiters of I(p), I(parent(p)) up to I(epsilon), the most specific prefix first. The first later waiter W on a prefix q that binds R(p minus q, x) and whose guard admits x fires in one COMM on I(q) with the vehicle of x, which is never stored. When x is not persistent and W does not peek, x is spent. When W is persistent, the send re-issues. Otherwise x stays in flight and the walk continues. When no waiter is left, STORE-MARKERS stores x on D(p) and one marker on each I(q) in depth order. A receive K on q takes the first datum d of CONSUME-CANDIDATES(q), in path-key order with the exact path first, that its pattern binds after re-rooting, and it checks the guard once. A refusal or no datum parks K on I(q). Otherwise one COMM on I(q) consumes the marker m_q(d) and binds the re-rooted payload. A persistent d keeps all of its markers, and a persistent K then re-issues as a divergent pair. A peek of a non-persistent d removes m_q(d) only and sends d again: at the fire cap the second send re-issues with m_q(d) pending, else it walks PRODUCE-WAITERS(p), and a waiter that does not peek spends d with RETIRE-PLAN, else m_q(d) is stored again with its identity. A receive that does not peek retires D(p):r and the markers on the other prefixes with single-channel K_retire COMMs, and a persistent K takes the next candidate in the next round.](diagrams/09-pathmap-delivery.svg)

*Figure 1. Source: [diagrams/09-pathmap-delivery.puml](diagrams/09-pathmap-delivery.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

## 3. The store

`PathMapOuter` is the outer store of a PathMap space (`spaces/outer/pathmap.rs:150-206`).
It names the channels of the space and checks the path rules.
It holds no state of its own, and the planner and the join engine stage every record.

### 3.1 Channels

*Table 1. The channels of a PathMap space.*

| Channel | Holds | Builder |
|---|---|---|
| $`D(\mathit{sid}, \mathrm{path\_key}(p))`$ | the datum records of the path $`p`$ | `path_data` (`spaces/outer/pathmap.rs:82-84`) |
| $`I(\mathit{sid}, \mathrm{prefix\_key}(q))`$ | one marker for each datum under $`q`$, and the waiters on $`q`$ | `index` (`spaces/outer/pathmap.rs:88-90`) |
| $`S(\mathit{sid})`$ | the token, the space counter | `token` (`spaces/outer/pathmap.rs:79`) |

All three are [storage channels](glossary.md#storage-channel) of the namespace tag 0xE0, so ordinary Rholang cannot name them.
`path_channels` gives the data channel of a path and the index channel of each of its prefixes in depth order (`spaces/outer/pathmap.rs:121-136`).
Each builder has a metered form that reserves its name template and its id buffer first.

### 3.2 Keys and the path rules

The key of a channel is the path key of its path (`spaces/outer/pathmap.rs:156-160`).
So `@c` and `@[c]` name one data channel, as rule PP-1 states.
Routing keys an operation by the canonical bytes of its channel, because routing runs before it knows the family of the space.
A PathMap operation does not use that routed key, because it derives every key from its channel.

`check_key` accepts a channel only when its path has at most 64 elements (`spaces/outer/pathmap.rs:167-176`).
No element of an accepted path holds an `EPathMap` or an `EZipper` at any depth.
A deeper path fails with `PathTooDeep` before the element scan (F9 of [decision 39](decision-register.md#decision-39)).
An element with a PathMap or a zipper fails with `UnsupportedPathElement`.
The check runs before any staging, so a failed operation changes no record.
Every prefix of an accepted path is accepted too, so every index channel of a stored datum is valid (`path_element_rules`, `PathMapPrefix.v:1455`).

### 3.3 Creation, names and the counter

The generic creation stages the space counter $`S(\mathit{sid})`$ with the value 0, and the store adds no other record (`spaces/outer/pathmap.rs:191-199`).
The counter channel of every path is the token (`spaces/outer/pathmap.rs:184-186`), so a PathMap space has no collection counter.
A PathMap space has no allocator.

The store makes fresh names as the HashMap store does (`spaces/outer/pathmap.rs:203-205`).
A fresh name `n` is an unforgeable channel, and its path is `[n]`.
So the paths under `[n]` form a private subtree that only the holders of `n` can name.

## 4. Path markers and the vehicle

### 4.1 The marker record

A path marker is a datum record on an index channel (`spaces/pathmap/markers.rs:143-156`).
The marker of the datum $`d`$ on the path $`p`$, for the prefix $`q \preceq p`$, has these parts.

*Table 2. The parts of the marker $`m_q(d)`$.*

| Part | Value | Reason |
|---|---|---|
| `pars` | `[EList[tag_I, GByteArray(src(d))], EList(p∖q)]` | the envelope names the datum, and the suffix gives the re-rooting |
| `tag_I` | `GByteArray([0xE0, 0x52, 0x49, 0x01])` (`spaces/pathmap/markers.rs:61`) | the layout of the record tags, with the kind `I` and the version 1 |
| `random_state` | $`\mathrm{split\_short}(\mathrm{derive}_{\mathtt{PathMarker}}(\mathrm{rs}(d)), \lvert q \rvert)`$ | distinct markers have distinct produce hashes |
| `persist` | the flag of $`d`$ | a persistent datum keeps its markers |
| `cost_authority` | the authority that the store of $`d`$ registers | the store of a marker is part of the store of $`d`$ |
| `cost_stack` | none | a marker carries no payload |

The marker tag has the layout of the record tags of `record.rs`, and this module defines it, so `RecordKind` does not change.
The derivation $`\mathrm{derive}_{\mathtt{PathMarker}}`$ splits the random state of $`d`$ by the namespace tag 0xE0 and then by the purpose byte 0x04 (`spaces/pathmap/markers.rs:128-138`).
The final split by the depth $`\lvert q \rvert`$ gives each marker of $`d`$ its own random state.
So no marker hash equals the hash of an older produce, which a merge would file as "copied by peek" (the plan, "Marker uniqueness").

### 4.2 The vehicle

The vehicle carries a datum to a waiter in a produce-side delivery (`spaces/pathmap/markers.rs:188-204`).
It is the marker of the datum, followed by the payload of the datum:

```math
v_q(d) = m_q(d).\mathit{pars} \mathbin{+\!\!+} \mathrm{payload}(d)
```

It has the random state, the cost authority and the cost stack of $`d`$.
The substrate never stores the incoming record of a fire, so no store keeps a vehicle.
The vehicle carries the payload so that its introduction covers the payload, as the introduction of the equivalent host produce does ([decision 40](decision-register.md#decision-40), Q3).
The stored markers stay short, because they carry no payload.

### 4.3 The marker invariant

The marker invariant MI holds in every state $`\sigma`$ of a space: the markers are exactly the markers of the stored datums.

```math
\mathrm{MI}(\sigma) \iff \mathrm{marks}(\sigma) = \biguplus_{d \in \mathrm{data}(\sigma)} \{\, m_q(d) : q \preceq \mathrm{path}(d) \,\}
```

The equation is an equation of multisets.
A datum on a path of depth $`n`$ has $`n + 1`$ markers, so the depth cap limits each datum to 65 markers.
The planner checks MI on each path that it reads, and a break fails closed with `Inconsistent` (`spaces/pathmap/delivery.rs:299-329`).

### 4.4 Strict decoders

The decoders accept only the exact image of their encoders (`spaces/pathmap/markers.rs:269-275`, `288-297`).
A marker has exactly two pars and no cost stack, and its envelope holds the marker tag and a source of 32 bytes.
Every other record is a `MalformedMarker`, and the operation fails with `Inconsistent`.
The theorem `marker_record_roundtrip` (`PathMapPrefix.v:1350`) proves that the decoder inverts the encoder and that the encoder is injective.

## 5. The delivery plans

The planner of `spaces/pathmap/delivery.rs` holds the pure parts of a delivery.
It reads a staged view that its caller passes in (`PrefixView`, `spaces/pathmap/delivery.rs:85-95`), and it stages nothing.
The engine of the space matches, checks guards, and stages with these plans.
Each plan below names a chunk in the style of literate programming ([Knu84](#ref-knu84)).

### 5.1 CONSUME-CANDIDATES

A receive on $`q`$ takes its candidates from the markers of $`I(q)`$ (`spaces/pathmap/delivery.rs:173-219`).
The candidates come path by path, in the byte order of the path keys, and the exact path comes first.
Inside one path, the order of the discipline applies.
It is the canonical order of DR-75 for a bag or a set, and the head for a head-only discipline.

```text
⟨CONSUME-CANDIDATES(q)⟩ ≡
  for each marker m on I(q):                         ▷ one marker for each datum under q
    (src, s) ← decode(m)
    key ← path_key(s)                                ▷ path_key(q ++ s) = prefix_key(q) ++ key
  group the markers by key, in byte order            ▷ the exact path (s = ε, key 0x00) first
  for each group with the suffix s, and p = q ++ s:
    pair each record of D(p) with its marker by src  ▷ MI, or Inconsistent
    order the records of D(p) by the discipline      ▷ DR-75, or the heads
  return the paths in key order, each with its records in that order
```

The order is the order `cand_lt` of the model (`spaces/pathmap/delivery.rs:532-538`):

```math
(p_1, r_1) <_{\mathrm{cand}} (p_2, r_2) \iff \mathrm{path\_key}(p_1) <_{\mathrm{lex}} \mathrm{path\_key}(p_2) \;\lor\; \bigl( p_1 = p_2 \land r_1 <_D r_2 \bigr)
```

Here $`r_1 <_D r_2`$ is the order of the discipline inside one path.
The terminator 0x00 is below every tag, so the exact path is the first candidate (`candidate_order_total`, `PathMapPrefix.v:1014`).

### 5.2 PRODUCE-WAITERS

A send on $`p`$ meets the waiters of the prefixes of $`p`$, the most specific prefix first (`spaces/pathmap/delivery.rs:338-380`).

```text
⟨PRODUCE-WAITERS(p)⟩ ≡
  for q in [p, parent(p), …, ε]:                     ▷ the depths |p|, |p| - 1, …, 0
    yield the waiters of I(q) in the waiter order of the discipline, with the suffix p∖q
```

Each prefix appears once, and no other prefix appears (`send_prefixes_spec` and `send_prefixes_depths` in `PathMapPrefix.v`).
A prefix without waiters gives nothing.

### 5.3 REROOT

The engine re-roots each argument of a candidate before it matches the argument (`spaces/pathmap/delivery.rs:385-403`).

```text
⟨REROOT(s, d)⟩ ≡
  R(s, v) for each argument v of d, with the random state, the cost authority
  and the cost stack of d                            ▷ a pattern matches the re-rooted datum
```

$`R`$ is the function of rule PP-5, and the adapter of rule PP-6 decodes and encodes each PathMap argument.

### 5.4 The send

A send of the datum $`x`$ on $`p`$ offers $`x`$ to PRODUCE-WAITERS($`p`$).
The first waiter that binds the re-rooted datum and whose guard admits it fires, and a refusal moves on ([decision 37](decision-register.md#decision-37)).

```text
⟨SEND(x, p)⟩ ≡
  check_key(p)                                       ▷ PathTooDeep, UnsupportedPathElement
  for each waiter W on q in PRODUCE-WAITERS(p):
    if W binds R(p∖q, x) and the guard of W admits it:
      COMM on I(q) with the vehicle v_q(x)           ▷ never stored
      if x is not persistent and W does not peek: return   ▷ x is spent
      if W is persistent: re-issue the send; return
      continue                                       ▷ x stays in flight (decision 30)
  STORE-MARKERS(x, p)
```

A fresh datum that a waiter spends was never stored, so the send leaves no record.
A peek waiter keeps the datum in flight, and the next waiter sees it ([decision 30](decision-register.md#decision-30)).

### 5.5 STORE-MARKERS

A send that no waiter spends stores its datum and then its markers (`spaces/pathmap/delivery.rs:409-436`).

```text
⟨STORE-MARKERS(x, p)⟩ ≡
  produce x on D(p)
  for k in 0 ..= |p|:                                ▷ depth order, ε first
    produce m_{prefix of depth k}(x) on I(prefix of depth k)
```

The events of a store are the produce on $`D(p)`$ and then one produce on each index channel, in depth order (the plan, "Events and merge").

### 5.6 The receive

A receive $`K`$ on $`q`$ takes the first candidate that its pattern binds, and it checks the guard once.
A refusal parks the receive ([decision 37](decision-register.md#decision-37)).

```text
⟨RECEIVE(K, q)⟩ ≡
  check_key(q)
  d ← the first datum of CONSUME-CANDIDATES(q) whose re-rooted payload K binds
  if no d, or the guard of K refuses d: park K on I(q); return
  COMM on I(q) over m_q(d), with R(p∖q, payload(d)) bound
  if d is persistent:                                ▷ d and its markers stay
    if K is persistent: re-issue K                   ▷ a divergent pair
    return
  if K peeks: ⟨SECOND-SEND(d, q)⟩; return
  RETIRE-PLAN(d, delivered = |q|)                    ▷ K_retire COMMs
  if K is persistent: K takes the next candidate in the next round
```

The COMM of a delivery consumes the marker $`m_q(d)`$, so the delivery and its retires stay on one channel each.

### 5.7 The second send of a peek

A consume-side peek removes the marker of its COMM, as the COMM of a host peek removes the record ([decision 45](decision-register.md#decision-45), Rule E).
The same transaction then sends the datum again, as the [second send](glossary.md#second-send) of the host does.
On a PathMap space the second send of a datum on $`p`$ is a send on $`p`$.

```text
⟨SECOND-SEND(d, q)⟩ ≡                                 ▷ the COMM removed m_q(d) only
  if the transaction made FIRE_ROUND_CAP fires:
    re-issue the second send, which carries m_q(d)   ▷ the marker is pending
    return
  for each waiter W on q′ in PRODUCE-WAITERS(path(d)):
    if W binds R(path(d)∖q′, d) and the guard of W admits it:
      COMM on I(q′) with the vehicle v_q′(d)
      if W does not peek:
        RETIRE-PLAN(d, delivered = |q|)              ▷ d is spent
        return
  store m_q(d) again with its identity               ▷ d is stored as before the peek
```

The second send has the identity and the cost authority of $`d`$, and it bumps no counter.
When no waiter spends $`d`$, the space holds the data and the markers that it held before the peek, up to their order (`peek_restores_delivered_marker`, `PathMapPrefix.v:2010`).
This is the PathMap form of RESTORE-SCAN and RETIRE-CLASS of the [scan driver](glossary.md#scan-driver): the record of $`d`$ and its markers form the class of $`d`$.

### 5.8 RETIRE-PLAN

A removal of a datum after a delivery retires its record and its other markers (`spaces/pathmap/delivery.rs:444-453`).

```text
⟨RETIRE-PLAN(d at p, delivered = k)⟩ ≡
  K_retire on D(p) for the record of d
  for j in 0 ..= |p|, j ≠ k:                          ▷ depth order
    K_retire on I(prefix of depth j) for m_{prefix of depth j}(d)
```

The marker of the depth $`k`$ is left out, because the delivery COMM removed it.
Each `K_retire` is a COMM on one channel (`delivery_vehicle_single_channel`, `PathMapPrefix.v:1911`).
The plan keeps MI: the record and its markers leave together (`marker_invariant_preserved`, `PathMapPrefix.v:2560`).

### 5.9 Head-only disciplines on prefixes

A queue, a stack, a cell and a priority queue match only their heads ([decision 11](decision-register.md#decision-11)).
On a PathMap space, a head waiter of $`I(q)`$ sees the head datum of each path under $`q`$.
So a change on $`D(p)`$ can make a head waiter fireable on any prefix of $`p`$, and a change on $`I(q)`$ only on $`q`$.

The engine marks those prefixes as dirty, and a cascade fires the dirty prefixes until no head waiter is fireable.
Each fire lowers a finite measure, so the cascade ends within the measure (`heads_no_match`, `PathMapPrefix.v:3038`).
The cascade is the DRAIN-AND-CASCADE step of the join engine, and Phase 2 runs it through the prefix source.

## 6. The token and the order of an operation

The token $`S(\mathit{sid})`$ orders the operations of the whole space.
An operation of an ordered discipline consumes the token with the value $`t`$, takes $`t`$ as its order, and produces $`t + 1`$ ([decision 31](decision-register.md#decision-31)):

```math
\mathrm{bump}(t) = (t,\; t + 1)
```

So the orders of the stored entries of a space are pairwise distinct and below the token.
A re-issued persistent operation keeps the order of its first round ([decision 43](decision-register.md#decision-43)), and a later bump never gives that order again.
A re-entered element therefore meets no equal order (`token_ord_reentry`, `PathMapPrefix.v:3126`).
It enters the head of a queue only when it is older than every stored entry.

A bag or a set keeps the order 0 in every round, as on a HashMap space.
A bag or a set peek bumps the token only when its second send changes the state ([decision 51](decision-register.md#decision-51), which amends F16 of [decision 39](decision-register.md#decision-39)).
Every other state change of a bag or a set bumps the token (the plan, "Per-space token").
So the writes under a shared prefix in two deploys form one chain, and a rejected chain leaves no orphan marker.

## 7. Merges and prefix contact

A merge of two branches runs the checks of the host on their event logs ([merge](glossary.md#merge)).
Check #2 reports a conflict when one branch creates a produce and the other branch creates a consume on one channel.

A new datum on $`p`$ produces its marker on $`I(q)`$ for every $`q \preceq p`$.
A new waiter on $`q`$ consumes on $`I(q)`$.
So a datum in one branch and a waiter on a prefix of its path in the other branch meet on $`I(q)`$.
Check #2 sees that contact (`prefix_contact_anchored`, `PathMapPrefix.v:1948`).
The index channels are injective, so a datum touches no index channel of a path that is not one of its prefixes.

The TLA+ model `PathMapPrefix.tla` checks the claim on every fork of two branches within its bounds (invariant `MergeDetectsPrefixContact`).
Its Unsafe control `SpaceWideIndexUnsafe` puts every marker on one space-wide channel $`I(\varepsilon)`$.
Then a datum on `[0, 1]` and a waiter on `[0]` share no channel, and TLC finds the missed contact.

A race on a datum that both branches read from the pre-state is a matter of check #1.
A waiter that persists across blocks makes the later writes under its prefixes trip check #3, as a join of the host does.

## 8. Metering

The planner reserves each read and each copy on the meter of the job before it makes it ([decision 42](decision-register.md#decision-42)).
It reserves each marker decode, each path-key encode, each key comparison, each suffix copy, and each buffer.
The orders of the disciplines reserve their own work, and the record builders have metered forms.

The trace model of the prefix scan proves the rule (`prefix_scan_trace_covered`, `PathMapPrefix.v:3268`).
Every prefix of the trace reads at most what it reserved, the reads after the commit included.
So a [host work](glossary.md#host-work) budget below the reservations fails at a reservation before the commit, and the operation has no effect.
The unit test `the_planner_reserves_its_work` cuts the budget at each step of the planner.

## 9. Failures

*Table 3. The failures of a PathMap operation. Each one happens before the commit, so the operation stages nothing.*

| Failure | Cause | Code |
|---|---|---|
| `PathTooDeep` | a channel path or a prefix with more than 64 elements (F9) | `spaces/outer/pathmap.rs:167-176` |
| `UnsupportedPathElement` | an element that holds an `EPathMap` or an `EZipper` at any depth | `spaces/outer/pathmap.rs:167-176` |
| `Inconsistent` | a malformed marker or vehicle, or a datum without its marker | `spaces/pathmap/markers.rs:88-95` |
| `HostWorkRejected` | a host-work budget that ends before a reservation | `spaces/pathmap/markers.rs:209-238` |

## 10. Security and determinism

**Unforgeable records.** The channels of a PathMap space are storage channels, so a program cannot produce on them.
Only the engine writes a marker, and the strict decoders reject every record that their encoders do not make.

**Bounded work.** The depth cap bounds the markers of a datum and the index channels of an operation to 65.
The meter reserves every read before it happens, and `FIRE_ROUND_CAP` bounds the fires of a transaction.

**Private subtrees.** A fresh name roots a subtree that no other program can name ([section 3.3](#33-creation-names-and-the-counter)).

**Determinism.** Every order of the planner is a function of the entries of its channels.
The planner uses the path keys, the source hashes and the orders of the disciplines.
It uses the list order of the store only as the last tie-break, as the canonical order of DR-75 does ([decision 41](decision-register.md#decision-41)).
So play and replay plan the same delivery and log the same events.

**Merge safety.** The markers make each prefix contact visible to check #2, and the token serializes the writes of one space.

## 11. Verification

The evidence follows the six acceptance categories of the campaign ([verification](verification.md)).
The Rocq module `PathMapPrefix.v` proves the headlines ([Roc25](#ref-roc25)).
The TLA+ module `PathMapPrefix.tla` checks the delivery and the merge with TLC ([Lam94](#ref-lam94), [YML99](#ref-yml99)).
The property tests run on the shared runner with a fixed seed ([CH00](#ref-ch00)).

### 11.1 Rocq headlines

The module holds the six headlines of wave W2 and the eleven headlines of wave W3.
Each headline has a `Print Assumptions` line, and `rocqchk` reports no axiom.

*Table 4. The headlines of wave W3 in `PathMapPrefix.v`.*

| Headline | Location | Statement | Property test |
|---|---|---|---|
| `marker_record_roundtrip` | `PathMapPrefix.v:1350` | the strict decoder inverts the encoder, the encoder is injective, and the tag is the record tag of the kind `I` | `prop_marker_record_roundtrip`, `the_marker_decoder_is_strict` |
| `vehicle_carries_payload` | `PathMapPrefix.v:1382` | the vehicle is the marker followed by the payload, and its size covers the payload | `prop_vehicle_carries_payload`, `a_vehicle_is_the_marker_followed_by_the_payload` |
| `path_element_rules` | `PathMapPrefix.v:1455` | the path rules, prefix closure, and at most 65 markers | `prop_path_element_rules`, `check_key_follows_the_path_rules` |
| `delivery_vehicle_single_channel` | `PathMapPrefix.v:1911` | every event is single-channel, and the vehicle is never stored | `a_removal_retires_the_record_and_every_other_marker`, `the_index_channels_of_the_model_are_the_store_channels`, and the event tests of Phase 2 |
| `prefix_contact_anchored` | `PathMapPrefix.v:1948` | the markers anchor every prefix contact on $`I(q)`$ | `prop_candidates_cover_the_prefix_in_key_order`, `the_path_channels_are_storage_channels_of_the_space` |
| `peek_restores_delivered_marker` | `PathMapPrefix.v:2010` | the second send restores the marker or spends the datum | `prop_marker_invariant_preserved`, `prop_waiter_prefixes_are_most_specific_first` |
| `no_match_remains` | `PathMapPrefix.v:2380` | the scan plans on prefixes leave no pending match without a guard | `prop_candidates_cover_the_prefix_in_key_order`, `prop_waiter_prefixes_are_most_specific_first`, and the plan tests of Phase 2 |
| `marker_invariant_preserved` | `PathMapPrefix.v:2560` | every staging and every plan keeps MI | `prop_marker_invariant_preserved`, `a_broken_marker_invariant_is_inconsistent` |
| `heads_no_match` | `PathMapPrefix.v:3038` | dirty coverage, and a cascade that ends with no fireable head | `head_orders_take_the_heads_of_a_path`, and the cascade tests of Phase 2 |
| `token_ord_reentry` | `PathMapPrefix.v:3126` | distinct orders, first-round orders of a re-issue, and no tie at a re-entry | `the_token_is_the_space_counter_and_the_counter_of_every_path`, and the order tests of Phase 2 |
| `prefix_scan_trace_covered` | `PathMapPrefix.v:3268` | the prefix scan reserves before it reads | `the_planner_reserves_its_work`, `metered_forms_equal_the_plain_forms_and_reject_a_cut_budget`, `the_metered_builders_reserve_their_work` |

The 37 negative controls of wave W3 each change one rule of the model, and each mutant fails inside its expected lemma.
The 17 controls of wave W2 still fail inside their lemmas on the extended module.

### 11.2 TLA+ configurations

*Table 5. The configurations of `PathMapPrefix.tla`. Each one checks `TypeOK`, `NoMatchRemains`, `MarkerCount` and `MergeDetectsPrefixContact`.*

| Configuration | Bounds | Distinct states | Result |
|---|---|---|---|
| `MC_PathMapPrefix.cfg` | four paths, all patterns, three operations | 341,931 at depth 4 | no violation |
| `MC_PathMapPrefix_Peek.cfg` | a chain of three paths, four operations | 51,074 at depth 5 | no violation |
| `MC_PathMapPrefix_Merge.cfg` | four paths, two operations, then a fork of two branches | 320,034 at depth 4 | no violation |

*Table 6. The Unsafe controls of `PathMapPrefix.tla`. Each one refutes exactly one invariant, and the run without that invariant finds no other violation.*

| Control | Rule that it reverts | Refuted invariant |
|---|---|---|
| `SpaceWideIndexUnsafe.cfg` | every marker on one space-wide index channel | `MergeDetectsPrefixContact` |
| `PathMapExactPathOnlyUnsafe.cfg` | a send meets the waiters of its exact path only | `NoMatchRemains` |
| `PathMapNoRetireUnsafe.cfg` | a delivery removes only the marker of its COMM | `MarkerCount` |
| `PathMapMarkerNotRestoredUnsafe.cfg` | the second send does not restore the marker of its COMM | `MarkerCount` |

### 11.3 Unit tests

The three modules hold 27 unit tests, 7 of them property tests.
The test `pathmap_segment_codec_matches_epm1_vectors` checks the channel ids of the store against the EPM1 transcription vectors.
The tests of wave W2 still pass, so the layer holds 60 unit tests in all.

### 11.4 Running the checks

1. Run the unit tests of the layer:

   ```text
   TMPDIR=$PWD/target/tmp cargo test -p rholang --lib -- spaces::pathmap spaces::outer::pathmap
   ```

2. Copy `PathMapPrefix.v` into a scratch directory under `target/`, then compile it there:

   ```text
   coqc -q -noglob PathMapPrefix.v
   rocqchk -o -R . '' PathMapPrefix
   ```

3. Run each configuration through `tlc_bounded` of `scripts/lib/tlc-run.sh`, with the Java temporary directory under `target/`.
4. Check each Unsafe control with `scripts/verify-reified/tlc-expect-violation.sh` and with its `--complement` option.

## 12. Status and open items

**Phase 1 (complete).** The store, the markers, the vehicle, the planner, the Rocq headlines, and the TLA+ model.

**Phase 2 (next).** The join engine first gets the store seams of joins plan section 9, and an independent review checks their design ([decision 51](decision-register.md#decision-51)).
Then the prefix source implements the source seam of the join engine for PathMap spaces.
Phase 2 also adds the dispatch of the PathMap families, their factory rows, and the alias `rho:space:PathMapSpace`.
The integration tests of `rholang/tests/reified_pathmap.rs` then run the delivery end to end, with replay and restart.

**Item O-1, closed: the token and a peek of a bag or a set.**
F16 says that a bag or a set peek does not bump the token.
Under Rule E, the second send of such a peek can fire a waiter that does not peek.
That fire removes the datum and the waiter.
[Decision 51](decision-register.md#decision-51) resolves the item: the peek bumps the token only when its second send fires such a waiter, because that fire changes the state.

**Open item O-2: a second send at the fire cap.**
A second send that meets a full transaction re-issues with the marker $`m_q(d)`$ that its COMM removed.
The re-issued operation must restore that marker or retire the datum, so the join engine must carry the marker with the re-issue.
Until the marker is stored again, MI holds for every other marker (`marker_invariant_preserved`).

## References

<a id="ref-ch00"></a>[CH00] K. Claessen and J. Hughes, "QuickCheck: a lightweight tool for random testing of Haskell programs," in *Proceedings of the Fifth ACM SIGPLAN International Conference on Functional Programming (ICFP 2000)*, ACM, 2000, pp. 268–279. [doi:10.1145/351240.351266](https://doi.org/10.1145/351240.351266)

<a id="ref-fips-rrs"></a>[FIPS-RRS] M. Stay and L. G. Meredith, "Reifying RSpaces," F1r3fly Improvement Proposal, 2025-09-26. [FIPS repository, approved/2025-09-26-Reifying-RSpaces](https://github.com/F1R3FLY-io/FIPS/blob/main/approved/2025-09-26-Reifying-RSpaces/Reifying%20RSpaces.md)

<a id="ref-gel85"></a>[Gel85] D. Gelernter, "Generative communication in Linda," *ACM Transactions on Programming Languages and Systems*, vol. 7, no. 1, pp. 80–112, 1985. [doi:10.1145/2363.2433](https://doi.org/10.1145/2363.2433)

<a id="ref-knu84"></a>[Knu84] D. E. Knuth, "Literate programming," *The Computer Journal*, vol. 27, no. 2, pp. 97–111, 1984. [doi:10.1093/comjnl/27.2.97](https://doi.org/10.1093/comjnl/27.2.97)

<a id="ref-lam94"></a>[Lam94] L. Lamport, "The temporal logic of actions," *ACM Transactions on Programming Languages and Systems*, vol. 16, no. 3, pp. 872–923, 1994. [doi:10.1145/177492.177726](https://doi.org/10.1145/177492.177726)

<a id="ref-mr05"></a>[MR05] L. G. Meredith and M. Radestock, "A reflective higher-order calculus," *Electronic Notes in Theoretical Computer Science*, vol. 141, no. 5, pp. 49–67, 2005. [doi:10.1016/j.entcs.2005.05.016](https://doi.org/10.1016/j.entcs.2005.05.016)

<a id="ref-roc25"></a>[Roc25] The Rocq Development Team, "The Rocq Prover," Zenodo, 2025. [doi:10.5281/zenodo.15149628](https://doi.org/10.5281/zenodo.15149628)

<a id="ref-yml99"></a>[YML99] Y. Yu, P. Manolios, and L. Lamport, "Model checking TLA+ specifications," in *Correct Hardware Design and Verification Methods (CHARME 1999)*, Lecture Notes in Computer Science, vol. 1703, Springer, 1999, pp. 54–66. [doi:10.1007/3-540-48153-2_6](https://doi.org/10.1007/3-540-48153-2_6)
