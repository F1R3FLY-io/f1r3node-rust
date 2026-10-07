---------------------- MODULE PreparedNativePhlo ----------------------
EXTENDS Integers

\* Bounded refinement of the offered-funded prepared handoff. The checked
\* frontend/caller owns the source bytes and authorization; RhoRuntime owns
\* source, wire, and item reservations plus the native rollback checkpoint.
\* Rust tests compare the concrete source and prepared metering dimensions.
VARIABLES actualSource, declaredSource, wireBytes, items, byteLimit,
          itemLimit, checked, runtimeReject, phase, verified, sourceCharges,
          wireCharged, itemsCharged, byteUsage, itemUsage, checkpoint,
          space, executed
vars == <<actualSource, declaredSource, wireBytes, items, byteLimit,
          itemLimit, checked, runtimeReject, phase, verified, sourceCharges,
          wireCharged, itemsCharged, byteUsage, itemUsage, checkpoint,
          space, executed>>
inputs == <<actualSource, declaredSource, wireBytes, items, byteLimit,
            itemLimit, checked, runtimeReject, checkpoint>>

Init ==
    /\ actualSource \in 0..2
    /\ declaredSource \in 0..2
    /\ wireBytes \in 0..2
    /\ items \in 0..2
    /\ byteLimit \in 0..4
    /\ itemLimit \in 0..2
    /\ checked \in BOOLEAN
    /\ runtimeReject \in BOOLEAN
    /\ checkpoint \in 0..1
    /\ phase = "admit"
    /\ verified = FALSE
    /\ sourceCharges = 0
    /\ wireCharged = FALSE
    /\ itemsCharged = FALSE
    /\ byteUsage = 0
    /\ itemUsage = 0
    /\ space = checkpoint
    /\ executed = FALSE

Admit ==
    /\ phase = "admit"
    /\ checked /\ declaredSource = actualSource
    /\ phase' = "source"
    /\ verified' = TRUE
    /\ UNCHANGED <<inputs, sourceCharges, wireCharged, itemsCharged,
                   byteUsage, itemUsage, space, executed>>

RejectUnverified ==
    /\ phase = "admit"
    /\ (~checked \/ declaredSource # actualSource)
    /\ phase' = "rejected"
    /\ UNCHANGED <<inputs, verified, sourceCharges, wireCharged, itemsCharged,
                   byteUsage, itemUsage, space, executed>>

ReserveSource ==
    /\ phase = "source"
    /\ byteUsage + declaredSource <= byteLimit
    /\ phase' = "wire"
    /\ sourceCharges' = sourceCharges + 1
    /\ byteUsage' = byteUsage + declaredSource
    /\ UNCHANGED <<inputs, verified, wireCharged, itemsCharged,
                   itemUsage, space, executed>>

RejectSource ==
    /\ phase = "source"
    /\ byteUsage + declaredSource > byteLimit
    /\ phase' = "rejected"
    /\ UNCHANGED <<inputs, verified, sourceCharges, wireCharged, itemsCharged,
                   byteUsage, itemUsage, space, executed>>

ReserveWire ==
    /\ phase = "wire"
    /\ byteUsage + wireBytes <= byteLimit
    /\ phase' = "items"
    /\ wireCharged' = TRUE
    /\ byteUsage' = byteUsage + wireBytes
    /\ UNCHANGED <<inputs, verified, sourceCharges, itemsCharged,
                   itemUsage, space, executed>>

RejectWire ==
    /\ phase = "wire"
    /\ byteUsage + wireBytes > byteLimit
    /\ phase' = "rejected"
    /\ UNCHANGED <<inputs, verified, sourceCharges, wireCharged, itemsCharged,
                   byteUsage, itemUsage, space, executed>>

ReserveItems ==
    /\ phase = "items"
    /\ itemUsage + items <= itemLimit
    /\ phase' = "execute"
    /\ itemsCharged' = TRUE
    /\ itemUsage' = itemUsage + items
    /\ UNCHANGED <<inputs, verified, sourceCharges, wireCharged,
                   byteUsage, space, executed>>

RejectItems ==
    /\ phase = "items"
    /\ itemUsage + items > itemLimit
    /\ phase' = "rejected"
    /\ UNCHANGED <<inputs, verified, sourceCharges, wireCharged, itemsCharged,
                   byteUsage, itemUsage, space, executed>>

Execute ==
    /\ phase = "execute"
    /\ phase' = "finish"
    /\ executed' = TRUE
    /\ space' = checkpoint + 1
    /\ UNCHANGED <<inputs, verified, sourceCharges, wireCharged, itemsCharged,
                   byteUsage, itemUsage>>

Finish ==
    /\ phase = "finish"
    /\ phase' = IF runtimeReject THEN "rejected" ELSE "accepted"
    /\ space' = IF runtimeReject THEN checkpoint ELSE space
    /\ UNCHANGED <<inputs, verified, sourceCharges, wireCharged, itemsCharged,
                   byteUsage, itemUsage, executed>>

Terminal == phase \in {"accepted", "rejected"} /\ UNCHANGED vars
Next == Admit \/ RejectUnverified \/ ReserveSource \/ RejectSource
        \/ ReserveWire \/ RejectWire \/ ReserveItems \/ RejectItems
        \/ Execute \/ Finish \/ Terminal
Spec == Init /\ [][Next]_vars

InvAuthorized == phase \in {"source", "wire", "items", "execute",
                              "finish", "accepted"} => verified /\ checked
InvExactSource == verified => declaredSource = actualSource
InvNoDoubleSourceCharge == sourceCharges <= 1
InvChargeOrder == wireCharged => sourceCharges = 1
InvRejectedRollback == phase = "rejected" => space = checkpoint
InvAcceptedExact == phase = "accepted" =>
    checked /\ executed /\ sourceCharges = 1 /\ wireCharged
    /\ itemsCharged /\ byteUsage = actualSource + wireBytes
    /\ itemUsage = items /\ space = checkpoint + 1
=======================================================================
