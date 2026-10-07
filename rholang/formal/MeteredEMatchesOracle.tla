----------------------- MODULE MeteredEMatchesOracle -----------------------
EXTENDS Naturals, Sequences

\* The oracle's bounded wrapper, before the existing metered matcher PDA.
\* Each reservation below abstracts one ordered reservation segment, not its
\* internal CloneBacking worklist events. MeteredMatcherPDA models the latter
\* matcher work. The clone implementation itself is the generated term_ops
\* explicit worklist, not the old recursive derived Clone.
CONSTANTS Verdict, BinderDepth
ASSUME Verdict \in BOOLEAN /\ BinderDepth \in Nat

ExpectedCharges == <<"context", "target-copy", "pattern-copy", "pda">>
ExpectedClones == <<"target", "pattern">>

VARIABLES initialCredit, credit, phase, charges, clones, result, depth
vars == <<initialCredit, credit, phase, charges, clones, result, depth>>

Init ==
    /\ initialCredit \in 0..4
    /\ credit = initialCredit
    /\ phase = "context"
    /\ charges = <<>>
    /\ clones = <<>>
    /\ result = "none"
    /\ depth = BinderDepth

ReserveContext ==
    /\ phase = "context" /\ credit > 0
    /\ credit' = credit - 1
    /\ charges' = Append(charges, "context")
    /\ phase' = "target-copy"
    /\ UNCHANGED <<initialCredit, clones, result, depth>>

ReserveTargetCopy ==
    /\ phase = "target-copy" /\ credit > 0
    /\ credit' = credit - 1
    /\ charges' = Append(charges, "target-copy")
    /\ phase' = "pattern-copy"
    /\ UNCHANGED <<initialCredit, clones, result, depth>>

ReservePatternCopy ==
    /\ phase = "pattern-copy" /\ credit > 0
    /\ credit' = credit - 1
    /\ charges' = Append(charges, "pattern-copy")
    /\ phase' = "clone-target"
    /\ UNCHANGED <<initialCredit, clones, result, depth>>

CloneTarget ==
    /\ phase = "clone-target"
    /\ clones' = Append(clones, "target")
    /\ phase' = "clone-pattern"
    /\ UNCHANGED <<initialCredit, credit, charges, result, depth>>

ClonePattern ==
    /\ phase = "clone-pattern"
    /\ clones' = Append(clones, "pattern")
    /\ phase' = "pda"
    /\ UNCHANGED <<initialCredit, credit, charges, result, depth>>

Match ==
    /\ phase = "pda" /\ credit > 0
    /\ credit' = credit - 1
    /\ charges' = Append(charges, "pda")
    /\ result' = Verdict
    /\ phase' = "done"
    /\ UNCHANGED <<initialCredit, clones, depth>>

Reject ==
    /\ phase \in {"context", "target-copy", "pattern-copy", "pda"}
    /\ credit = 0
    /\ result' = "rejected"
    /\ phase' = "rejected"
    /\ UNCHANGED <<initialCredit, credit, charges, clones, depth>>

Terminal == phase \in {"done", "rejected"} /\ UNCHANGED vars
Next == ReserveContext \/ ReserveTargetCopy \/ ReservePatternCopy
        \/ CloneTarget \/ ClonePattern \/ Match \/ Reject \/ Terminal
Spec == Init /\ [][Next]_vars

InvCredit == credit + Len(charges) = initialCredit
InvChargeOrder == charges = SubSeq(ExpectedCharges, 1, Len(charges))
InvCloneOrder == clones = SubSeq(ExpectedClones, 1, Len(clones))
InvNoUnreservedClone == Len(clones) > 0 => Len(charges) >= 3
InvNoPrematureVerdict == phase = "done" =>
    charges = ExpectedCharges /\ clones = ExpectedClones /\ result = Verdict
InvTypedRejection == phase = "rejected" => result = "rejected"
InvBinderDepth == depth = BinderDepth
=============================================================================
