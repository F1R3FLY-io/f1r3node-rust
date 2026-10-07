---------------------- MODULE RejectionReasonConfluence ----------------------
EXTENDS FiniteSets

CONSTANT UseCanonicalJoin

Unspecified == "unspecified"
Collateral == "collateral_chain_drop"
MergeConflict == "merge_conflict"
Duplicate == "duplicate_occurrence"
Reasons == {Unspecified, Collateral, MergeConflict, Duplicate}
Causes == Reasons \ {Unspecified}

Join(left, right) ==
    IF left = Duplicate \/ right = Duplicate
    THEN Duplicate
    ELSE IF left = MergeConflict \/ right = MergeConflict
         THEN MergeConflict
         ELSE IF left = Collateral \/ right = Collateral
              THEN Collateral
              ELSE Unspecified

UpdateReason(current, observed) ==
    IF UseCanonicalJoin THEN Join(current, observed) ELSE observed

VARIABLES
    observedA,
    observedB,
    reasonA,
    reasonB

vars == <<observedA, observedB, reasonA, reasonB>>

Init ==
    /\ observedA = {}
    /\ observedB = {}
    /\ reasonA = Unspecified
    /\ reasonB = Unspecified

ObserveA(cause) ==
    /\ cause \in Causes \ observedA
    /\ observedA' = observedA \union {cause}
    /\ reasonA' = UpdateReason(reasonA, cause)
    /\ UNCHANGED <<observedB, reasonB>>

ObserveB(cause) ==
    /\ cause \in Causes \ observedB
    /\ observedB' = observedB \union {cause}
    /\ reasonB' = UpdateReason(reasonB, cause)
    /\ UNCHANGED <<observedA, reasonA>>

Next ==
    \/ \E cause \in Causes : ObserveA(cause)
    \/ \E cause \in Causes : ObserveB(cause)

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ observedA \subseteq Causes
    /\ observedB \subseteq Causes
    /\ reasonA \in Reasons
    /\ reasonB \in Reasons

Inv_EqualObservationConverges ==
    observedA = observedB => reasonA = reasonB

Inv_DuplicateDominates ==
    /\ Duplicate \in observedA => reasonA = Duplicate
    /\ Duplicate \in observedB => reasonB = Duplicate

Inv_MergeDominatesCollateral ==
    /\ MergeConflict \in observedA /\ Duplicate \notin observedA => reasonA = MergeConflict
    /\ MergeConflict \in observedB /\ Duplicate \notin observedB => reasonB = MergeConflict

Inv_CollateralIsFallback ==
    /\ observedA = {Collateral} => reasonA = Collateral
    /\ observedB = {Collateral} => reasonB = Collateral

=============================================================================
