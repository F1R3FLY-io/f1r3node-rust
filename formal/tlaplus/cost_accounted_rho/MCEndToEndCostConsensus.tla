------------------------ MODULE MCEndToEndCostConsensus ------------------------
EXTENDS EndToEndCostConsensus

AuthoritiesDef == {"alice", "bob"}
EventsDef == {1, 2, 3}
InitialSupplyDef == [a \in AuthoritiesDef |-> IF a = "alice" THEN 4 ELSE 3]
MismatchedSupplyDef == [a \in AuthoritiesDef |-> IF a = "alice" THEN 5 ELSE 3]
AbsentSupplyDef == [a \in AuthoritiesDef |-> 0]
CostReservationDef == [a \in AuthoritiesDef |-> 2]
EventDebitDef ==
  [e \in EventsDef |->
    [a \in AuthoritiesDef |->
      IF e = 1 /\ a = "alice" THEN 1
      ELSE IF e = 2 THEN 1
      ELSE IF e = 3 /\ a = "bob" THEN 1
      ELSE 0]]
FeeDebitDef == [a \in AuthoritiesDef |-> IF a = "alice" THEN 1 ELSE 0]
ExecutionChoicesDef == {{1, 2}, {2, 3}, {2}}
DeploymentKindsDef == {"client", "validator-heartbeat", "validator-dummy"}
ParentOrdersDef == {<<"left", "right">>, <<"right", "left">>}

=============================================================================
