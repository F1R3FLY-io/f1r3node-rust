--------------------------- MODULE RecoveryLeader ---------------------------
EXTENDS Naturals

CONSTANTS ValidatorCount, ViewDependent

VARIABLES lfbA, lfbB

vars == <<lfbA, lfbB>>
Validators == 1..ValidatorCount

StableLeader == CHOOSE v \in Validators : \A w \in Validators : v <= w
ViewLeader(lfb) == IF ViewDependent THEN 1 + (lfb % ValidatorCount) ELSE StableLeader

Init == /\ lfbA \in 0..ValidatorCount
        /\ lfbB \in 0..ValidatorCount

ChangeViews == \E a, b \in 0..ValidatorCount : /\ lfbA' = a
                                                  /\ lfbB' = b

Next == ChangeViews
Spec == Init /\ [][Next]_vars

TypeOK == /\ lfbA \in 0..ValidatorCount
          /\ lfbB \in 0..ValidatorCount
          /\ ViewLeader(lfbA) \in Validators
          /\ ViewLeader(lfbB) \in Validators

Inv_CrossViewLeader == ViewLeader(lfbA) = ViewLeader(lfbB)
=============================================================================
