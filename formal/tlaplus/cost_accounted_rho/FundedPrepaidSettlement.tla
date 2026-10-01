---------------------- MODULE FundedPrepaidSettlement ----------------------
EXTENDS Naturals, FiniteSets

CONSTANTS Workers, Price, SkipConsumption, PartialRollback, ChargePlatform

Kinds == {"Success", "UserFailure", "PlatformFailure"}
Phases == {"Ready", "Wallet", "Popped", "Receipts", "Done", "Failed"}
Retain(k) == k # "PlatformFailure"
NewCells(k) == IF k = "Success" THEN 1 ELSE 0
UsedCells(k) == IF Retain(k) THEN 1 ELSE 0
Debit(k) == IF Retain(k) THEN 1 + Price * (1 + NewCells(k)) ELSE 0
Initial == [wallet |-> 10, oldCells |-> 2, oldReceipts |-> 2, newReceipts |-> 0]
Expected(k) == [wallet |-> 10 - Debit(k),
                oldCells |-> 2 - UsedCells(k),
                oldReceipts |-> 2 - UsedCells(k), newReceipts |-> NewCells(k)]

VARIABLES phase, kind, candidate, published
vars == <<phase, kind, candidate, published>>

Init ==
  /\ kind \in [Workers -> Kinds]
  /\ phase = [w \in Workers |-> "Ready"]
  /\ candidate = [w \in Workers |-> Initial]
  /\ published = [w \in Workers |-> Initial]

Wallet(w) ==
  /\ phase[w] = "Ready"
  /\ phase' = [phase EXCEPT ![w] = "Wallet"]
  /\ candidate' = [candidate EXCEPT
       ![w].wallet = 10 - IF ChargePlatform /\ kind[w] = "PlatformFailure"
                          THEN 1 ELSE Debit(kind[w]),
       ![w].newReceipts = NewCells(kind[w])]
  /\ UNCHANGED <<kind, published>>

Pop(w) ==
  /\ phase[w] = "Wallet"
  /\ phase' = [phase EXCEPT ![w] = "Popped"]
  /\ candidate' = [candidate EXCEPT
       ![w].oldCells = IF SkipConsumption THEN @ ELSE 2 - UsedCells(kind[w])]
  /\ UNCHANGED <<kind, published>>

MoveReceipts(w) ==
  /\ phase[w] = "Popped"
  /\ phase' = [phase EXCEPT ![w] = "Receipts"]
  /\ candidate' = [candidate EXCEPT ![w].oldReceipts = 2 - UsedCells(kind[w])]
  /\ UNCHANGED <<kind, published>>

Commit(w) ==
  /\ phase[w] = "Receipts"
  /\ phase' = [phase EXCEPT ![w] = "Done"]
  /\ published' = [published EXCEPT ![w] = candidate[w]]
  /\ UNCHANGED <<kind, candidate>>

Fail(w) ==
  /\ phase[w] \in {"Wallet", "Popped", "Receipts"}
  /\ phase' = [phase EXCEPT ![w] = "Failed"]
  /\ candidate' = IF PartialRollback THEN [candidate EXCEPT
       ![w].oldCells = Initial.oldCells, ![w].oldReceipts = Initial.oldReceipts]
     ELSE [candidate EXCEPT ![w] = Initial]
  /\ UNCHANGED <<kind, published>>

Retry(w) ==
  /\ phase[w] = "Failed"
  /\ phase' = [phase EXCEPT ![w] = "Ready"]
  /\ UNCHANGED <<kind, candidate, published>>

Next == \E w \in Workers : Wallet(w) \/ Pop(w) \/ MoveReceipts(w) \/ Commit(w) \/ Fail(w) \/ Retry(w)
Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ Price \in 0..4
  /\ kind \in [Workers -> Kinds]
  /\ phase \in [Workers -> Phases]
  /\ candidate \in [Workers -> [wallet : 0..10, oldCells : 0..2, oldReceipts : 0..2, newReceipts : 0..1]]
  /\ published \in [Workers -> [wallet : 0..10, oldCells : 0..2, oldReceipts : 0..2, newReceipts : 0..1]]
ExactCommittedSettlement == \A w \in Workers : phase[w] = "Done" => published[w] = Expected(kind[w])
PublishedPrepaidBacking == \A w \in Workers : published[w].oldCells = published[w].oldReceipts
PlatformPreservesFunding == \A w \in Workers : kind[w] = "PlatformFailure" => published[w] = Initial
PrivateUntilCommit == \A w \in Workers : phase[w] # "Done" => published[w] = Initial
CompleteFailureRollback == \A w \in Workers : phase[w] \in {"Ready", "Failed"} => candidate[w] = Initial
SameOutcomeAgreement == \A a, b \in Workers :
  phase[a] = "Done" /\ phase[b] = "Done" /\ kind[a] = kind[b] => published[a] = published[b]
IndependentCandidatesEnabled == \A w \in Workers : phase[w] = "Ready" => ENABLED Wallet(w)
=============================================================================
