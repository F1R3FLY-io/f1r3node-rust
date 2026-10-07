---------------- MODULE NormalizerContinuationHandoff ----------------
EXTENDS Naturals

\* A bounded abstraction of normalize_drive's Combine lifecycle. `slot`
\* represents its single outstanding child/value obligation. Descend
\* continues the same continuation; Handoff completes it and starts a new
\* continuation with a fresh arity. The body's work remains iterative.
\* Init represents an entry Done/Descend/Tail that has already scheduled its
\* first child; Handoff is a Combine-only transition, never an entry step.
VARIABLES arity, filled, slot, handoffs, last, parentWasComplete
vars == <<arity, filled, slot, handoffs, last, parentWasComplete>>

Init ==
    /\ arity \in 1..3
    /\ filled = 0
    /\ slot = "child"
    /\ handoffs = 0
    /\ last = "init"
    /\ parentWasComplete = FALSE

Absorb ==
    /\ slot = "child" /\ filled < arity
    /\ filled' = filled + 1
    /\ slot' = "value"
    /\ last' = "absorb"
    /\ parentWasComplete' = FALSE
    /\ UNCHANGED <<arity, handoffs>>

Descend ==
    /\ slot = "value" /\ filled < arity
    /\ slot' = "child"
    /\ last' = "descend"
    /\ parentWasComplete' = FALSE
    /\ UNCHANGED <<arity, filled, handoffs>>

Handoff ==
    /\ slot = "value" /\ filled = arity /\ handoffs < 2
    /\ \E nextArity \in 1..3:
           /\ arity' = nextArity
           /\ filled' = 0
           /\ slot' = "child"
           /\ handoffs' = handoffs + 1
           /\ last' = "handoff"
           /\ parentWasComplete' = TRUE

Done ==
    /\ slot = "value" /\ filled = arity
    /\ slot' = "done"
    /\ last' = "done"
    /\ parentWasComplete' = TRUE
    /\ UNCHANGED <<arity, filled, handoffs>>

Tail ==
    /\ slot = "value" /\ filled = arity
    /\ slot' = "done"
    /\ last' = "tail"
    /\ parentWasComplete' = TRUE
    /\ UNCHANGED <<arity, filled, handoffs>>

Terminal == slot = "done" /\ UNCHANGED vars
Next == Absorb \/ Descend \/ Handoff \/ Done \/ Tail \/ Terminal
Spec == Init /\ [][Next]_vars

InvSlot ==
    /\ arity \in 1..3
    /\ filled \in 0..arity
    /\ slot \in {"child", "value", "done"}
InvNoFinalDescend == last = "descend" => filled < arity
InvTypedHandoff == last = "handoff" =>
    slot = "child" /\ filled = 0 /\ parentWasComplete
InvCompletedParent == last \in {"done", "tail"} =>
    slot = "done" /\ filled = arity /\ parentWasComplete
InvOneOutstanding == slot # "done" =>
    (slot = "child" \/ slot = "value")
======================================================================
