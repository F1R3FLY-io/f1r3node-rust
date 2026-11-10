# Garbage Collection of Unforgeable Names

## Status

Opt-in only. Set `F1R3FLY_SCOPE_TRAPPED_GC=1` in the runtime's environment
to enable the sweep inside `RhoRuntimeImpl::create_checkpoint`. The
analysis, the sweep infrastructure, and the test suite ship as
foundation for future work, but the sweep stays off by default pending a
soak against the full system-contract corpus. See
[Limitations](#limitations-and-known-incomplete) for the specific
interaction that forced this gating.

The H1 invariant is machine-checked against bounded Rholang programs in
`formal/tlaplus/scope_trapped_gc/` — the post-fix TLC run is clean over
835 distinct states (depth-2 term universe), and two registered negative
controls (`payload_leak` and `body_mention`) exhibit exactly the
invariants the strict Rust classifier rules out. The universal, axiom-
free proof is the Phase 2 Rocq mechanization tracked in
`docs/rholang/garbage-collection-formalization-plan.md`.

## Context

A Rholang name `N` is a **garbage name** when no future process can send on `N`
or receive on `N`. A process is a **garbage process** when it sends or receives
only on garbage names. A garbage process consumes rspace storage without any
chance of further communication.

Today the rspace tuplespace removes an entry only when a COMM event fires
(`rspace++/src/rspace/rspace/ops_produce.rs:130`). Every other entry remains.
Non-matching entries become permanent state at the next checkpoint, when
`create_checkpoint` writes the hot-store delta into the radix trie whose root
is committed to consensus (`rspace++/src/rspace/rspace/ispace_impl.rs:41`).

This document specifies a conservative heuristic that proves some fresh
unforgeable names garbage, and that lets rspace delete their stored
sends and continuations at the deploy boundary.

## Terminology

| Term | Meaning |
|---|---|
| **Forgeable name** | A quoted process `@P` whose underlying `Par` has only forgeable constituents. Any process can construct a forgeable name. `@1`, `@"s"`, `@[1,2]` are forgeable. |
| **Unforgeable name** | A `GUnforgeable` payload — `GPrivate`, `GDeployId`, `GDeployerId`, or `GSysAuthToken`. A deploy cannot construct the 32-byte id of a `GPrivate` minted by another deploy. |
| **Fresh unforgeable** | A `GPrivate` whose 32-byte id was produced by `Blake2b512Random::next()` during `eval_new`. |
| **Channel position** | A syntactic position in a `Par` whose value serves as the channel of a send or the source of a receive: `Send.chan`, `Receive.binds[*].source`, or a `Bundle.body` nested transitively inside one of those. |
| **Payload position** | Any position that carries a name as a value: `Send.data[*]`, `Receive.binds[*].patterns[*]`, `Match.target`, `Match.cases[*].pattern`, any `Expr` subterm, any `Connective`, the body of a bundle that is itself a value (not in a channel position). |

The user's three examples set the sanity targets:

1. `@1` is not garbage: any process can construct `@1` and send on it.
2. `new x in { x!(5) }` — the fresh `N` bound to `x` is garbage. No other
   process has `N`, nothing else sends on `N`, and the one parked send
   on `N` will never match.
3. `new x in { @1!(*x) }` — the fresh `N` is not garbage, because
   `*x` in `Send.data` lets any process that does
   `for (z <- @1) { P }` bind `z` to `N`.

## Heuristic H1: Strict Scope-Trapped Unforgeables

A fresh `GPrivate` `N`, minted by `new x in P` at `reduce.rs:eval_new`
(line 1348), is **scope-trapped in P** when every syntactic occurrence of
the bound variable `x` in the normalized body `P` lies in a *top-level*
channel position — reachable through par-composition, bundle channels,
nested `new.p`, match-case sources, and if-branches (constructs that
reduce immediately rather than park a body in rspace).

Any mention of `x` inside a `Receive.body` disqualifies trappedness,
even when the receive's source is `x` itself. Receive bodies are stored
as parked continuations; a persistent receive combined with a nested
receive on a non-trapped channel can promote the inner mention into a
fresh send/receive outside the closure once the enclosing receive fires.
The walk short-circuits as safe whenever the subterm's cached
`locally_free` bitset has a zero bit at the target index.

### Safety Theorem (H1)

**If `N` is scope-trapped in `P` at the reduction of `new x in P`, then
in every reduction state reachable from `P[N/x]`, `N` appears only in a
top-level channel position (through par-composition, bundle wrappers,
nested `new.p`, match-case sources, or if-branches). In particular, `N`
never ends up inside a parked `Receive.body`, so firing an unrelated
receive on a non-trapped channel cannot promote a reference to `N` into
a fresh send/receive.**

**Proof (induction on reduction steps).**

Let `R` be the set of reduction states reachable from `P[N/x]`. We prove
that in every state `S ∈ R`, every occurrence of `N` is in a top-level
channel position inside `S`.

- **Base.** `S = P[N/x]`. Capture-avoiding substitution preserves
  syntactic position. By scope-trappedness, every occurrence of `x` in
  `P` is in a top-level channel position (never inside a `Receive.body`,
  pattern, expression, or payload), so every occurrence of `N` in
  `P[N/x]` is likewise top-level channel.

- **Step.** Suppose the claim holds for `S`, and `S → S'` is one
  reduction step. The Rholang reduction rules that can introduce a new
  occurrence of `N` are:

  1. **β-substitution from a pattern match.** A receive
     `for (q_1, ..., q_n <- c) { K }` matches a send `c!(d_1, ..., d_n)`;
     the continuation `K` is substituted with each `q_i := d_i`. For a
     new occurrence of `N` to appear via `K[d_i/q_i]`, some `d_i` must
     contain `N` — but by hypothesis `N` appears only in channel
     positions, not in payloads, so no `d_i` contains `N`. Hence the
     step introduces no new `N` occurrences in `S'`, and surviving
     occurrences retain their structural positions.

  2. **Firing a parked receive.** A matched receive `for (…) <- c {K}`
     emits its body `K` into the reduction. For `K` to introduce an
     occurrence of `N`, the receive's body must have mentioned `N` —
     but strict scope-trappedness forbids `N` inside any `Receive.body`,
     so `K` cannot mention `N`. The receive's own channel `c` may or
     may not be `N`; either way the firing produces no new `N`
     occurrence outside the existing closure.

  3. **Match reduction.** `match t { p_i => K_i; … }` binds the pattern
     variables of the matching case from `t`. Introducing `N` would
     require `t` or some `p_i` to contain `N`; both are payload
     positions forbidden by the hypothesis.

  4. **`new y in Q` reduction.** Allocates a fresh `M ≠ N`
     (`Blake2b512Random::next` yields a new 32-byte id); introduces no
     `N`.

  5. **COMM event.** Removes a parked send and continuation on some
     channel; cannot introduce any new term.

  No other rule changes the set of occurrences of `N`. The claim holds
  in `S'`.

By induction the claim holds in every `S ∈ R`. QED.

### Corollary: Sweep Correctness

Once the deploy's reduction graph has quiesced, any rspace entry keyed on a
scope-trapped `N` is deadlocked forever: its complementary entry would need
to come from a process that holds `N` as a value, but no such process can
exist by the theorem. Deleting the entry removes a deadlocked rendezvous
and changes no future observable behavior.

### Cross-Deploy Argument

`GPrivate.id` is 32 bytes from `Blake2b512Random::next()`
(`rholang/src/rust/interpreter/reduce.rs:1381`,
 `crypto/src/rust/hash/blake2b512_random.rs:169`), seeded from
deploy-unique material. A later deploy cannot produce the same bytes without
inverting Blake2b-512. Hence a scope-trapped `N` from deploy `D` cannot
re-enter via any deploy `D'`, and the induction above closes across deploy
boundaries.

## Position Classification Rationale

The walk calls a position **safe** only when a name in that position becomes
the *channel of a send or receive at runtime*, with no intermediate
computation that could duplicate or publish it.

| Position | Classification | Why |
|---|---|---|
| `Send.chan`, `Receive.binds[*].source` | safe | Consumed as the channel of a send or receive. |
| `Bundle.body` nested in a channel position | safe | A bundle on a channel is a wrapper that restricts polarity; the underlying value stays in a channel role. |
| `Send.data[*]` | unsafe | Becomes the matched datum `A` in a COMM, which binds it into the continuation's scope. |
| `Receive.binds[*].patterns[*]` | unsafe | A name reference in a pattern is matched against the sender's datum; a successful match binds the datum into the continuation scope. |
| `Match.target`, `Match.cases[*].pattern` | unsafe | Same pattern-binding argument as `Receive.patterns`. |
| `Expr` subterm (incl. `If.condition`) | unsafe | Expressions compute. An `EVar` can serve as a channel reference only when the channel position itself normalizes to `Par{ exprs: [EVar] }`; everywhere else the expression context is a value context. |
| `Connective` | unsafe | Pattern context. |
| `Bundle.body` as a value (not in a channel position) | unsafe | The bundle is a *value* and a receiver can `match` the bundle to extract the body. |

The walk is index-targeted: it asks only "does variable index `idx` appear
only in safe positions?". It uses `bitset_contains(&term.locally_free, idx)`
as the first short-circuit on every structural step, so it never descends
into a subtree that provably does not mention `idx`.

## Correspondence to Code

| Concept | File | Entry |
|---|---|---|
| Static analysis `is_scope_trapped(idx, par)` | `rholang/src/rust/interpreter/garbage.rs` | top |
| Trait `ChannelKeyProbe` | `rspace++/src/rspace/rspace_interface.rs` | near top |
| `impl ChannelKeyProbe for Par` | `rholang/src/rust/interpreter/garbage.rs` | end |
| Fresh id generation | `rholang/src/rust/interpreter/reduce.rs:1381` | `rand.next()` inside `eval_new` |
| Trapped-set field on reducer | `rholang/src/rust/interpreter/reduce.rs:302` | `trapped_gprivates` on `DebruijnInterpreter` |
| Analysis invocation | `rholang/src/rust/interpreter/reduce.rs:eval_new` | before `alloc` |
| ISpace sweep entry | `rspace++/src/rspace/rspace_interface.rs` | `sweep_trapped` |
| Sweep implementation | `rspace++/src/rspace/rspace/ops_sweep.rs` | `locked_sweep` |
| Checkpoint wiring | `rholang/src/rust/interpreter/rho_runtime.rs:create_checkpoint` | call `sweep_trapped` before `space.create_checkpoint` |
| Replay invariance | `casper/src/rust/rholang/replay_runtime.rs` | replay reaches the same `create_checkpoint`, deterministic sweep. |

## Limitations and Known Incomplete

**Why the sweep is opt-in.** During development the casper
multi-parent-block regression tests (for example
`multi_parent_casper_should_estimate_parent_properly`) fail when the
sweep runs over the full genesis + block-production flow, because the
sweep removes rspace entries that a later system deploy (such as
`CloseBlock` → `PoS!("closeBlock", …)`) depends on. H1 is sound in the
Rholang-only sense formalized above, but the Rust runtime has an
additional, out-of-syntax side channel: callers of
`RuntimeOps::capture_results_with_name`, `play_exploratory_par`, etc.,
predict fresh `GPrivate` ids from the deploy rng and read directly from
`get_data_par`/`get_waiting_continuations`. Those readers can reach a
"scope-trapped" entry even though the Rholang analysis proved no process
can. Soaking H1 against every system-deploy and runtime-inspector path
before flipping the default on is future work.

**False negatives are the design choice.** Example:

```rholang
new x in {
  @"u"!(*x) |
  for (y <- @"u") { for (z <- y) { Nil } }
}
```

Semantically the only process that will ever receive `*x` is the local
`for (y <- @"u")`, and the only use of `y` in the continuation is as a
channel. `N` is ultimately garbage. H1 rejects this case because `*x` is in
a `Send.data` position; proving it safe would require an escape analysis
across processes.

**System unforgeables are excluded by construction.** `GDeployId`,
`GDeployerId`, and `GSysAuthToken` enter `eval_new` by the URN-injection
branch, not `rand.next()`. The analysis records only the indices of
rand-allocated binders.

**Replay must stay bit-exact.** The analysis is a pure function of the
normalized `Par` AST and the binder index. The trapped-set is drained at the
same deploy-boundary events in both play and replay. The sweep sorts
channel keys lexicographically (a `C: Ord` bound already required by
rspace) before issuing removes.

**Installed continuations and installed joins stay in place.** The existing
`remove_continuation` at `rspace++/src/rspace/hot_store.rs:572` guards
installed slot 0 and is a no-op there; our sweep reuses that path and
therefore cannot delete an installed entry.

**Soft checkpoint revert.** A deploy that fails or is reverted drains the
trapped-set so ids from an aborted deploy cannot be swept against a
successor deploy's hot store.

## Future Extensions (Separate Design Docs)

- **H2: Bundle0 wrapping.** An occurrence of `x` wrapped only in
  `bundle0{ *x }` is read- and write-locked in principle; a stronger,
  still-conservative heuristic could allow this position as safe. Needs
  care around `match` deconstruction of bundles.
- **H3: Mark-and-sweep reachability from forgeable roots.** A deploy-end
  pass could classify any name not reachable from forgeable channels or
  system unforgeables as garbage. Requires determinism analysis and a
  reachability graph over all stored datums and continuations.
- **H4: Flow-sensitive escape analysis.** Prove that a received name is
  only ever used in channel position in the receiver's continuation. Covers
  the user's example 3's "u" shape when the receiver is trivial.

## Verification

- `cargo test -p rholang garbage::` — unit battery on `is_scope_trapped`.
- `cargo test -p rholang scope_trapped_` — deploy-through-checkpoint
  integration tests.
- `cargo test -p casper replay_determinism_scope_trapped` — play-vs-replay
  state-root equality.
- Manual: `just run-standalone` with a deploy of `new x in { x!(5) }`.
  Observe the sweep's tracing span and confirm the resulting state root
  matches the state root of an equivalent empty deploy.
