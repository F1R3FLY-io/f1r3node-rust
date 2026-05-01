---
title: Garbage collection of Rholang names — formal design
status: draft
author: claude-session
date: 2026-05-01
related-docs:
  - docs/plans/rholang-gc-isabelle.md
  - docs/rholang/02-syntax-reference.md
  - docs/rholang/08-channels-and-concurrency.md
  - docs/rholang/10-system-channels.md
  - docs/plans/where-clauses-and-match-guards-2026-04-29.md
related-code:
  - models/src/main/protobuf/RhoTypes.proto
  - rholang/src/rust/interpreter/reduce.rs
  - rholang/src/rust/interpreter/matcher/spatial_matcher.rs
  - rspace++/src/rspace/match.rs
  - rho-pure-eval/src/lib.rs
---

# Garbage collection of Rholang names — design

## 1. Problem

Rholang names are first-class. They are produced by `new x in P` (an
unforgeable atom), by quotation `@P` (any process is a name), by deploy-time
ambients (`GDeployId`, `GDeployerId`, `GSysAuthToken`), and by URIs binding
system processes (`rho:io:stdout`, `rho:registry:lookup`, `rho:crypto:*`,
…). Synchronization happens at a name when a `Send` and a `Receive` (linear,
persistent, or peek; possibly inside a multi-bind join) match through the
spatial matcher and any attached `where` guard evaluates to `true`.

We want to identify names on which **no future synchronization can ever
occur**, no matter what processes are added to the running shard. This is
the classical garbage-collection question for the π-calculus, but Rholang
forces us to be explicit about reflection, bundles, persistence variants,
and the existence of publicly-known unforgeables (system channels).

## 2. Definitions

### 2.1 Names and unforgeable atoms

We model Rholang names following `RhoTypes.proto`:

```
Name ::= GPrivate(atom)        — fresh atom from `new`
       | GDeployId(bytes)      — ambient at deploy time
       | GDeployerId(bytes)    — ambient at deploy time
       | GSysAuthToken         — ambient (system authority)
       | GUri(uri)             — fixed system channel name
       | Quote(P)              — @P, with P : Par
       | Bundle(cap, n)        — bundle±0 wrapping a name; cap ∈ {R, W, RW, ⊥}
```

The set of **atoms** of a name `n`, written `atoms(n)`, is the set of
`GPrivate(a)` atoms occurring anywhere inside `n`, including under
quotations and bundles, computed by structural recursion over `n` and into
the `Par` carried by any `Quote(P)`.

A fixed set `pub` enumerates the **publicly-known unforgeables**: system
URIs (`rho:io:stdout`, the cryptographic primitives, registry, etc.), the
deploy-time ambients (`GDeployerId`, `GSysAuthToken`), and `GDeployId`s of
deploys that have already been observed by the adversary. `pub` is part of
the model parameter; it captures what every context in the network can be
assumed to know.

### 2.2 Forgeability

Given a context `K`, the name `c` is **K-forgeable** iff every atom of `c`
either occurs in `atoms(K)` or is in `pub`. Otherwise `c` is
**K-unforgeable**: at least one of its atoms is private to `P`'s `new`
binders and α-converted out of `K`'s reach.

### 2.3 Synchronization observable

We model the runtime configuration as a pair `(σ, P)` where `σ` is a
multiset of datums and waiting continuations, mirroring rspace's hot store
(see `rspace++/src/rspace/internal.rs`, `Datum`, `WaitingContinuation`).
The reduction relation `(σ, P) → (σ', P')` includes the standard
structural rules and a single observable rule **COMM(c)** that fires when:

1. there is a waiting continuation on (a join including) channel `c` whose
   patterns spatially match available datums on the joined channels, AND
2. the receive's `where` guard (if any) evaluates to `GBool(true)` under
   the bound substitution.

This is what `rspace++/src/rspace/match.rs::check_commit` (lines 71–83)
records as a `CommProto` in the audit log.

### 2.4 Garbage

Let `K[P]` denote a context plug followed by the standard α-convention
that renames `P`'s `new`-bound atoms fresh w.r.t. `K`. A name `c` is
**garbage with respect to `P`** iff for every context `K` such that `c` is
K-unforgeable (after α-convention), and every reduction sequence
`(σ₀, K[P]) →* (σ', Q')`, no step in the sequence is a `COMM(c)` step.

Equivalently: no shard configuration extending `P` can ever synchronize on
`c`.

This refinement of the user-stated definition is **not** vacuous: when `c`
is K-unforgeable, `K` cannot syntactically mention `c`, so `K`-internal
synchronizations on `c` are ruled out and the only way a `COMM(c)` can
arise is through `P` (possibly after extruding part of `c`'s atom-content).

## 3. Algorithms

We give two sound (under-approximating) decision procedures.

### 3.1 GC₀ — coarse algorithm, sufficient for non-triviality

```
gc₀(P) = { c : Name | ∃ a ∈ atoms(c).
                       a ∉ atoms(P) ∧ a ∉ pub ∧ a ∉ bn_new(P) }
```

where `bn_new(P)` is the set of atoms bound by `new` declarations syntactically
in `P` (after a canonical α-renaming). The condition says: `c` mentions an
atom that is neither known to `P` nor in `pub` nor allocatable by `P`'s
`new`s. By construction the universe of atoms is countably infinite while
`atoms(P) ∪ pub ∪ bn_new(P)` is finite (or, for `pub`, fixed and finite),
so `gc₀(P)` is co-finite ⇒ infinite ⇒ nonempty.

GC₀ is the workhorse of the non-triviality theorem; it is too weak to be
useful at runtime because it never reports anything that occurs in `P`.

### 3.2 GC₁ — escape and one-sided analysis

GC₁ also collects `new`-bound atoms `u` of `P` that satisfy:

- **(escape)** `u` does not occur as a sub-term of any payload of any
  `Send` reachable in `P`, where reachability includes traversal through
  `@`-quotations and through bundles whose write capability is open;
- **(one-sided)** `u` appears in `P` either only in send-channel
  positions, only in receive-channel positions, or not at all as a
  sync-channel; and
- **(bundle-aware refinement)** if every occurrence of `u` as a
  sync-channel is wrapped under `bundle+ ·` (read-only by holders) then
  `u` is garbage when `P` has no internal send on `u`; symmetrically for
  `bundle-` and missing receives. `bundle0` rules out both sides.

Examples GC₁ catches:

| Process | Why `u` (the atom) is garbage |
|---|---|
| `new x in { x!(0) }` | `x` doesn't escape; only sends on `x` |
| `new x in { for(_ <- x){ 0 } }` | `x` doesn't escape; only receives on `x` |
| `new x in { @{*x \| "tag"}!(0) }` | `x` does not appear as a sync-channel |
| `new x in { bundle+{x}!(0) }` | `bundle+` makes `x` read-only outside; no internal send via `bundle+` ⇒ no sync |

GC₁ can be tightened further with may/must analysis of `Match`, `If`, and
persistent receives, but the three rules above are sufficient for the
soundness theorem we state below.

### 3.3 Treatment of patterns and `where` guards

For the soundness proofs we treat the spatial matcher and `rho-pure-eval`
(see `rho-pure-eval/src/lib.rs`) as oracles that may always succeed. This
is a sound over-approximation of the runtime: any concrete match/guard can
only *fewer* `COMM(c)` events fire, which can only enlarge the actual
garbage set, so GC₀/GC₁ remain sound under the abstraction.

If a future Phase-1' refinement wants tighter results (e.g. a name
syntactically receivable on but blocked by an unsatisfiable guard), it can
specialize the oracle; the current scope keeps the matcher abstract.

## 4. Theorems

We state the three target theorems precisely.

**Theorem (Soundness of GC₀).** For all `P` and all `c ∈ gc₀(P)`, `c` is
garbage with respect to `P`.

**Theorem (Non-triviality).** For all `P`, the set `gc₀(P)` is countably
infinite, hence nonempty.

**Theorem (Soundness of GC₁).** For all `P` and all `c ∈ gc₁(P)`, `c` is
garbage with respect to `P`. Strictly extends GC₀.

The chain `gc₀(P) ⊆ gc₁(P) ⊆ true_garbage(P)` is the structure of the
proof: GC₀ provides non-triviality cheaply; GC₁ provides the runtime-useful
algorithm.

## 5. Correspondence with the Rust interpreter

Each modeled construct in the Isabelle development carries a citation to
the Rust source it tracks. The mechanization does **not** include a
formal adequacy proof (refinement or extracted code) — that is out of
scope for this branch — but the design fixes the correspondence
operationally:

| Isabelle rule / definition | Rust source (file:line) |
|---|---|
| `Par` syntax | `models/src/main/protobuf/RhoTypes.proto:34–247` |
| `Name = GPrivate \| GDeployId \| GDeployerId \| GSysAuthToken \| GUri \| Quote \| Bundle` | `RhoTypes.proto:528–552` (`GUnforgeable` oneof) |
| `new` allocates fresh atom | `rholang/src/rust/interpreter/reduce.rs:1168–1310` (`eval_new`) |
| SEND rule | `rholang/src/rust/interpreter/reduce.rs:912–954` |
| RECEIVE rule (linear/persistent/peek/join) | `rholang/src/rust/interpreter/reduce.rs:955–1052` |
| MATCH rule (with guard fall-through) | `rholang/src/rust/interpreter/reduce.rs:1053–1135` |
| IF rule (synchronous type error) | `rholang/src/rust/interpreter/reduce.rs:1136–1167` |
| Spatial matching oracle | `rholang/src/rust/interpreter/matcher/spatial_matcher.rs` |
| Where-guard commit (cross-channel) | `rspace++/src/rspace/match.rs:71–83` |
| Pure expression evaluator (oracle for guards) | `rho-pure-eval/src/lib.rs` |
| `σ` configuration (datums / waiting continuations) | `rspace++/src/rspace/internal.rs` |
| `CommProto` (the COMM observable) | `models/src/main/protobuf/RSpacePlusPlusTypes.proto:258–263` |
| System channel `pub` | `rholang/src/rust/interpreter/system_processes.rs:86–144` (`FixedChannels`) |

The "Adequacy.thy" theory restates this table as Isabelle comments and
asserts no theorems; it exists so a future refinement proof can pin its
obligations to identified rule-by-rule statements.

### 5.1 Conservative abstractions made by the model

1. **Patterns are oracles.** The Isabelle `matches` relation is left as a
   parameter: any `(pattern, target)` pair can be declared a match. This
   is sound for over-approximating COMM events (see §3.3).
2. **`rho-pure-eval` is an oracle.** Any `Par` may be declared to
   evaluate to `GBool(true)`. Sound for the same reason.
3. **Numerics are uninterpreted.** Six numeric types appear in the
   syntax for completeness, but their semantics is opaque — none of them
   produce `GPrivate` atoms.
4. **Method calls and string operations are opaque value transformers.**
   They can rearrange data but cannot produce new unforgeable atoms
   (`reduce.rs` confirms only `eval_new` allocates `GPrivate`).
5. **Replay determinism is not modeled.** The Rust runtime allocates
   `GPrivate` deterministically from `Blake2b512Random`; for GC purposes
   we only need that fresh atoms are distinct from every previously
   visible atom, which the abstract `Atoms.thy` provides.

These abstractions only enlarge the set of behaviors the proof has to
quantify over, so GC₀/GC₁ soundness against the abstract semantics
implies soundness against any refinement.

## 6. What is in and out of scope for this branch

In scope (Phase 0, this branch):

- The design above.
- Isabelle theory skeleton with full datatypes, definitions, and theorem
  statements. Proof bodies are `sorry`.
- An explicit phased plan (`docs/plans/rholang-gc-isabelle.md`).
- An epoch entry in `docs/ToDos.md`.

Out of scope (later phases):

- Discharging the `sorry`s (Phase 1).
- Differential testing of Isabelle vs Rust traces (Phase 2).
- Wiring a runtime GC pass into `rspace++` based on GC₁ (Phase 3).
- Formal adequacy theorem against extracted Rust (deferred indefinitely).

## 7. Modeling notes and open questions

- **Nominal2 vs locally-nameless.** Nominal2 is the natural choice for
  π-calculus binders; we adopt it. Locally-nameless would also work but
  duplicates substitution machinery.
- **Status of `pub` over time.** `pub` is treated as a constant for the
  Phase-0 statements. A more refined model would let `pub` grow as the
  adversary observes deploy data; the proofs go through unchanged so long
  as `bn_new(P)` and `pub` remain disjoint at the moment GC₀ is
  evaluated.
- **Bundle composition.** `Bundle(cap₁, Bundle(cap₂, n))` reduces to
  `Bundle(cap₁ ⊓ cap₂, n)` per Rholang's bundle algebra. The model
  records this; GC₁'s bundle-aware refinement uses the meet.
- **Persistent and peek receives.** Their treatment in GC₁ is uniform —
  the question is only whether *any* sync on `c` ever fires, so
  multiplicity does not matter for the soundness theorem. It will matter
  later for runtime cost estimates.
