--------------------- MODULE NativePhloExecutionCeiling ---------------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS Workers, Limit, StaleReservation, ResetWhileActive

Events == {1, 2, 3, 4}
Identities == {1, 2, 3}
Identity(e) == CASE e = 1 -> 1 [] e = 2 -> 1 [] e = 3 -> 2 [] OTHER -> 3
Compute(e) == IF e = 3 THEN 0 ELSE 1
Transfer(e) == IF e = 2 THEN 2 ELSE 1
Owners(e) == IF e = 4 THEN 0 ELSE 3
Weight(e) == (2 * Compute(e) + Transfer(e)) * Owners(e)
Total(receipts) ==
  (IF receipts[1] = 0 THEN 0 ELSE Weight(receipts[1])) +
  (IF receipts[2] = 0 THEN 0 ELSE Weight(receipts[2])) +
  (IF receipts[3] = 0 THEN 0 ELSE Weight(receipts[3]))

VARIABLES request, phase, observed, epoch, used, receipts, preparedEpoch
vars == <<request, phase, observed, epoch, used, receipts, preparedEpoch>>

Init ==
  /\ request \in [Workers -> Events]
  /\ phase = [w \in Workers |-> "Ready"]
  /\ observed = [w \in Workers |-> 0]
  /\ preparedEpoch = [w \in Workers |-> 0]
  /\ epoch = 0
  /\ used = 0
  /\ receipts = [i \in Identities |-> 0]

Prepare(w) ==
  /\ phase[w] = "Ready"
  /\ phase' = [phase EXCEPT ![w] = "Prepared"]
  /\ observed' = [observed EXCEPT ![w] = used]
  /\ preparedEpoch' = [preparedEpoch EXCEPT ![w] = epoch]
  /\ UNCHANGED <<request, epoch, used, receipts>>

Commit(w) ==
  LET e == request[w]
      id == Identity(e)
      base == IF StaleReservation THEN observed[w] ELSE used
      fresh == receipts[id] = 0
      fits == base + Weight(e) <= Limit
  IN
  /\ phase[w] = "Prepared"
  /\ phase' = [phase EXCEPT ![w] = "Done"]
  /\ used' = IF fresh /\ fits THEN base + Weight(e) ELSE used
  /\ receipts' = IF fresh /\ fits THEN [receipts EXCEPT ![id] = e] ELSE receipts
  /\ UNCHANGED <<request, observed, epoch, preparedEpoch>>

Cancel(w) ==
  /\ phase[w] \in {"Ready", "Prepared"}
  /\ phase' = [phase EXCEPT ![w] = "Done"]
  /\ UNCHANGED <<request, observed, epoch, used, receipts, preparedEpoch>>

Reset ==
  /\ epoch = 0
  /\ ResetWhileActive \/ (\A w \in Workers : phase[w] = "Done")
  /\ epoch' = 1
  /\ used' = 0
  /\ receipts' = [i \in Identities |-> 0]
  /\ phase' = IF ResetWhileActive THEN phase ELSE [w \in Workers |-> "Ready"]
  /\ UNCHANGED <<request, observed, preparedEpoch>>

Next == (\E w \in Workers : Prepare(w) \/ Commit(w) \/ Cancel(w)) \/ Reset
Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ phase \in [Workers -> {"Ready", "Prepared", "Done"}]
  /\ receipts \in [Identities -> Events \cup {0}]
  /\ used \in 0..Limit
  /\ epoch \in 0..1
ExactAcceptedUsage == used = Total(receipts)
HardCeiling == Total(receipts) <= Limit
IdentityPreserved == \A i \in Identities : receipts[i] # 0 => Identity(receipts[i]) = i
NoCrossGenerationWorker == \A w \in Workers : phase[w] = "Prepared" => preparedEpoch[w] = epoch
ParallelPreparation == \A w \in Workers : phase[w] = "Ready" => ENABLED Prepare(w)
=============================================================================
