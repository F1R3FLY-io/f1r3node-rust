# Scope-Trapped Unforgeable GC — bounded safety model

Companion claim: `docs/rholang/garbage-collection.md`.
Formalization plan: `docs/rholang/garbage-collection-formalization-plan.md`.

Mechanizes the H1 safety invariant for the scope-trapped unforgeable sweep
shipped in PR #984 on a bounded, Rholang-shaped operational semantics. Each
initial program under test is a `new x in P` whose body `P` is drawn from a
small finite universe; TLC explores every reachable state of the live
process plus parked rspace entries and checks that the fresh unforgeable
`N` never ends up in a payload position or any `Receive` body reachable
from outside the closure.

## Model ↔ code

| Model operator / variable | Rust code |
|---|---|
| `IsScopeTrapped(x, P)` | `rholang/src/rust/interpreter/garbage.rs::is_scope_trapped` (the `check_par` walk) |
| `AllocFresh` action | `rholang/src/rust/interpreter/reduce.rs::eval_new` (per-binder `rand.next()` ⇒ `GPrivate.id`) |
| `ParkOne` action | `rspace++/src/rspace/rspace/ops_consume.rs::locked_consume`, `ops_produce.rs::locked_produce` (store a continuation / datum when no counterpart matches) |
| `Comm` action | `rspace++/src/rspace/rspace/ops_produce.rs::process_match_found` (COMM firing + `remove_matched_datum_and_join`) |
| `ExtFire` action | External-deploy send into a forgeable parked receive — the transition Rule (B) of the H1 induction; models how a non-trapped receive body can run from outside the closure. |
| `program`, `parkData`, `parkCont` | Live Rholang term + rspace single-channel data / continuations. Joins are out of scope for the TLA+ pass; they're covered by the Rocq pass. |
| Classifier knobs `H1_RejectsPayload`, `H1_RejectsReceiveBody` | The two rejection clauses in `is_scope_trapped` (payload-position and any-mention-in-receive-body). Each pre-fix config unsets one. |

## Configurations

| Config | Knobs | Expected | Shows |
|---|---|---|---|
| `MC_ScopeTrappedGC` | `H1_RejectsPayload = TRUE`, `H1_RejectsReceiveBody = TRUE` | **clean** | H1 as shipped. Every reachable state over the depth-2 body universe keeps `N` out of payloads, parked data, and parked receive bodies on forgeable channels. |
| `MC_ScopeTrappedGC_payload_leak_pre_fix` | `H1_RejectsPayload = FALSE` | `Inv_N_NeverInParkedData` violated | Accepting `x` in a `Send.data` lets a parked datum on a forgeable channel carry `N` as its payload; any external receiver binds it and leaks `N`. |
| `MC_ScopeTrappedGC_body_mention_pre_fix` | `H1_RejectsReceiveBody = FALSE` | `Inv_N_NeverInParkedBodyOnForgeable` violated | Accepting `x` anywhere inside a receive body lets an external deploy park a continuation on a forgeable channel whose body textually contains `N`; firing re-emits `N` outside the closure. This is the reduction rule the original prose proof forgot. |

The pre-fix configs are the formal-side counterexamples; CI runs all three.

## Running locally

```bash
java -jar ~/.tla/tla2tools.jar -workers auto \
  -config MC_ScopeTrappedGC.cfg MC_ScopeTrappedGC.tla
```

Each pre-fix config is invoked the same way; `check-tla-invariants.sh`
asserts a specific invariant violation for each.

## Implementation obligations

What the bounded model gates for the Rust side (follows the project's
"spec must expose defects" rule):

1. **Payload rejection stays in `is_scope_trapped`.** Removing the
   `Send.data` arm reproduces the `MC_ScopeTrappedGC_payload_leak_pre_fix`
   counterexample.
2. **Full `Receive.body` rejection stays in.** Any relaxation (even the
   "receive-body-on-x" relaxation that the holed prose proof thought was
   safe) reproduces one of the pre-fix counterexamples.
3. **The sweep acts only on the trapped set.** Sweeping entries keyed on
   channels outside `trapped_gprivates` is not modeled here (it would be
   obviously unsound regardless of H1).

Deep treatment — invariant proofs, open questions, Rocq handoff — lives
in `docs/rholang/garbage-collection.md` and the formalization plan at
`docs/rholang/garbage-collection-formalization-plan.md`.
