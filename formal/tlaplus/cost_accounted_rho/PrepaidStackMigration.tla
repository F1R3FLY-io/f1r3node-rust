----------------------- MODULE PrepaidStackMigration -----------------------
EXTENDS Naturals, Sequences, FiniteSets, TLC

CONSTANTS Validators, PublishEarly, OmitTailRecord, PartialRollback

Origins == {"a", "b", "c", "d"}
Sources == 1..3
InitialCells == [id \in Origins |->
  CASE id \in {"a", "b"} -> <<1, 2, 3>>
    [] id = "c" -> <<2, 3>>
    [] OTHER -> <<3>>]
ConsumedCells == [id \in Origins |->
  CASE id = "a" -> <<1>>
    [] id = "b" -> <<1, 2>>
    [] id = "c" -> <<2>>
    [] OTHER -> <<>>]
RemainingCells == [id \in Origins |->
  SubSeq(InitialCells[id], Len(ConsumedCells[id]) + 1, Len(InitialCells[id]))]
PhysicalCounts(cells) == [source \in Sources |->
  Cardinality({id \in Origins : Len(cells[id]) = source})]
InitialPhysical == PhysicalCounts(InitialCells)
FinalPhysical == PhysicalCounts(RemainingCells)

VARIABLES phase, physical, receipts, publishedPhysical, publishedReceipts
vars == <<phase, physical, receipts, publishedPhysical, publishedReceipts>>

Init ==
  /\ phase = [v \in Validators |-> "Ready"]
  /\ physical = [v \in Validators |-> InitialPhysical]
  /\ receipts = [v \in Validators |-> InitialCells]
  /\ publishedPhysical = physical
  /\ publishedReceipts = receipts

Pop(v) ==
  /\ phase[v] = "Ready"
  /\ phase' = [phase EXCEPT ![v] = "Popped"]
  /\ physical' = [physical EXCEPT ![v] = FinalPhysical]
  /\ UNCHANGED <<receipts, publishedPhysical, publishedReceipts>>

MoveReceipts(v) ==
  /\ phase[v] = "Popped"
  /\ phase' = [phase EXCEPT ![v] = "Moved"]
  /\ receipts' = [receipts EXCEPT ![v] =
       IF OmitTailRecord THEN [RemainingCells EXCEPT !["b"] = <<>>]
       ELSE RemainingCells]
  /\ UNCHANGED <<physical, publishedPhysical, publishedReceipts>>

Commit(v) ==
  /\ phase[v] = "Moved" \/ (PublishEarly /\ phase[v] = "Popped")
  /\ phase' = [phase EXCEPT ![v] = "Done"]
  /\ publishedPhysical' = [publishedPhysical EXCEPT ![v] = physical[v]]
  /\ publishedReceipts' = [publishedReceipts EXCEPT ![v] = receipts[v]]
  /\ UNCHANGED <<physical, receipts>>

Fail(v) ==
  /\ phase[v] \in {"Popped", "Moved"}
  /\ phase' = [phase EXCEPT ![v] = "Failed"]
  /\ physical' = IF PartialRollback THEN physical
                  ELSE [physical EXCEPT ![v] = InitialPhysical]
  /\ receipts' = [receipts EXCEPT ![v] = InitialCells]
  /\ UNCHANGED <<publishedPhysical, publishedReceipts>>

Retry(v) ==
  /\ phase[v] = "Failed"
  /\ phase' = [phase EXCEPT ![v] = "Ready"]
  /\ UNCHANGED <<physical, receipts, publishedPhysical, publishedReceipts>>

Next == \E v \in Validators : Pop(v) \/ MoveReceipts(v) \/ Commit(v) \/ Fail(v) \/ Retry(v)
Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ phase \in [Validators -> {"Ready", "Popped", "Moved", "Done", "Failed"}]
  /\ physical \in [Validators -> [Sources -> 0..4]]
  /\ publishedPhysical \in [Validators -> [Sources -> 0..4]]
  /\ receipts \in [Validators -> [Origins -> Seq(1..3)]]
  /\ publishedReceipts \in [Validators -> [Origins -> Seq(1..3)]]

PublishedPhysicalBinding == \A v \in Validators :
  publishedPhysical[v] = PhysicalCounts(publishedReceipts[v])

CommittedCellConservation == \A v \in Validators :
  phase[v] = "Done" => \A id \in Origins :
    ConsumedCells[id] \o publishedReceipts[v][id] = InitialCells[id]

FailedCandidateRestored == \A v \in Validators :
  phase[v] = "Failed" =>
    /\ physical[v] = InitialPhysical
    /\ receipts[v] = InitialCells

UncommittedCandidateIsPrivate == \A v \in Validators :
  phase[v] # "Done" =>
    /\ publishedPhysical[v] = InitialPhysical
    /\ publishedReceipts[v] = InitialCells

IndependentValidatorsAgree == \A left, right \in Validators :
  phase[left] = "Done" /\ phase[right] = "Done" =>
    /\ publishedPhysical[left] = publishedPhysical[right]
    /\ publishedReceipts[left] = publishedReceipts[right]
=============================================================================
