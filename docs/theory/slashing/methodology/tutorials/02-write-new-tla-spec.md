# 02 · Write a new TLA⁺ spec + TLC model

## 1 · Prerequisites

- TLA⁺ syntax familiarity (Lamport, *Specifying Systems* [Lam02]).
- Knowledge of the existing seven slashing specs
  ([`formal/tlaplus/slashing/`](../../../../../formal/tlaplus/slashing/)).
- TLC installed; `tlc` available on `PATH`.

## 2 · Skeleton

Create `formal/tlaplus/slashing/<Name>.tla`:

```tla
--------------------------------- MODULE <Name> ---------------------------------
(****************************************************************************)
(* Finite-state model of <component>.                                       *)
(*                                                                          *)
(* Models:                                                                  *)
(*   - <list states>                                                        *)
(*   - <list actions>                                                       *)
(*   - <list invariants>                                                    *)
(*                                                                          *)
(* Complements the Rocq mechanization at                                    *)
(*   formal/rocq/slashing/theories/<Module>.v                               *)
(*                                                                          *)
(* Reference: docs/theory/slashing/slashing-verification.md §<N>.           *)
(****************************************************************************)

EXTENDS Integers, Sequences, FiniteSets, TLC

CONSTANTS
    <Param1>,
    <Param2>

VARIABLES
    <Var1>,
    <Var2>

vars == <<<Var1>, <Var2>>>

TypeOK ==
    /\ <type predicate>

Init ==
    /\ <initialize>

<Action1>(<args>) ==
    /\ <precondition>
    /\ <update primed vars>
    /\ UNCHANGED <<<unchanged vars>>>

Next == \E <args> : <Action1>(<args>) \/ <Action2>(<args>)

Spec == Init /\ [][Next]_vars

Inv_<Property> ==
    /\ <invariant>

THEOREM Spec => []Inv_<Property>

=============================================================================
```

Create the corresponding `MC_<Name>.tla`:

```tla
--------------------------------- MODULE MC_<Name> ---------------------------------
EXTENDS <Name>

CONSTANT <ParamConcrete> = {<concrete values>}

INSTANCE <Name> WITH <Param1> <- <ParamConcrete>

=============================================================================
```

And `MC_<Name>.cfg`:

```cfg
SPECIFICATION Spec
INVARIANT Inv_<Property>
CONSTANTS
    <Param1> = <ParamConcrete>
    <Param2> = <ConcreteValue>
```

## 3 · Example from this repo

See [`formal/tlaplus/slashing/EquivocationDetector.tla`](../../../../../formal/tlaplus/slashing/EquivocationDetector.tla)
— a complete detector LTS with eight invariants.

## 4 · Verification step

```
tlc -workers 12 MC_<Name>.tla
```

Expected output ends with:

```
Model checking completed. No error has been found.
```

If a violation is found, TLC prints the trace from `Init` to the
violating state. The trace is the witness; classify it via the
witness rule in
[`../pipeline/01-witness-to-source-rule.md`](../pipeline/01-witness-to-source-rule.md).

## 5 · Common pitfalls

- **State-space explosion** — calibrate `<ParamConcrete>` so the
  reachable state space is ≤ 10⁵; see
  [`../formal-methods/02-model-checking-tla.md §6.3`](../formal-methods/02-model-checking-tla.md).
- **Missing fairness** — liveness invariants (`P ~> Q`) require
  `WF_vars(Action)` or `SF_vars(Action)` clauses.
- **Weak `Init`** — start from the most permissive well-formed
  state, not a hand-crafted "interesting" state.
