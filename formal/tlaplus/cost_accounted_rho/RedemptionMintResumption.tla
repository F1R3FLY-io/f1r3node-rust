-------------------- MODULE RedemptionMintResumption --------------------
EXTENDS Integers, FiniteSets

CONSTANT Defect

ASSUME Defect \in {
  "None",
  "DeleteAfterReplayEpoch",
  "SnapshotEligibility",
  "IgnoreLedger",
  "MintWhileHalted",
  "UnrecordedCredit"
}

Validators == {"A", "B"}
Epochs == 0..2
Workers == {"A1", "A2", "B1"}
WorkerStates == {"Idle", "Begun", "Done"}
Phases == {"Active", "Quarantined", "Burned"}
NoValidator == "NoValidator"
NoEpoch == -1

Target == [w \in Workers |-> IF w = "B1" THEN "B" ELSE "A"]
MintKey(v, e) == [validator |-> v, epoch |-> e]
MintKeyType == [validator : Validators, epoch : Epochs]

InitialMinted == {
  MintKey("A", 0), MintKey("A", 1), MintKey("B", 0)
}

VARIABLES
  currentEpoch,
  phase,
  halted,
  minted,
  everMinted,
  credited,
  workerState,
  workerValidator,
  workerEpoch,
  workerWasEligible,
  historyDeleted,
  ineligibleCredit,
  unrecordedCredit

vars == <<
  currentEpoch, phase, halted, minted, everMinted, credited,
  workerState, workerValidator, workerEpoch, workerWasEligible,
  historyDeleted, ineligibleCredit, unrecordedCredit
>>

Eligible(v, e) ==
  /\ phase[v] = "Active"
  /\ v \notin halted
  /\ MintKey(v, e) \notin minted

Init ==
  /\ currentEpoch = 1
  /\ phase = [v \in Validators |-> IF v = "A" THEN "Quarantined" ELSE "Active"]
  /\ halted = {"A"}
  /\ minted = InitialMinted
  /\ everMinted = InitialMinted
  /\ credited =
       [v \in Validators |->
         [e \in Epochs |-> IF MintKey(v, e) \in InitialMinted THEN 1 ELSE 0]]
  /\ workerState = [w \in Workers |-> "Idle"]
  /\ workerValidator = [w \in Workers |-> NoValidator]
  /\ workerEpoch = [w \in Workers |-> NoEpoch]
  /\ workerWasEligible = [w \in Workers |-> FALSE]
  /\ historyDeleted = FALSE
  /\ ineligibleCredit = FALSE
  /\ unrecordedCredit = FALSE

Redeem(v, replayEpoch) ==
  /\ v \in Validators
  /\ replayEpoch \in Epochs
  /\ replayEpoch <= currentEpoch
  /\ phase[v] = "Quarantined"
  /\ phase' = [phase EXCEPT ![v] = "Active"]
  /\ halted' = halted \ {v}
  /\ minted' =
       IF Defect = "DeleteAfterReplayEpoch"
       THEN {key \in minted : key.validator /= v \/ key.epoch <= replayEpoch}
       ELSE minted
  /\ historyDeleted' = (historyDeleted \/ minted' /= minted)
  /\ UNCHANGED <<currentEpoch, everMinted, credited, workerState,
                  workerValidator, workerEpoch, workerWasEligible,
                  ineligibleCredit, unrecordedCredit>>

Slash(v) ==
  /\ phase[v] = "Active"
  /\ phase' = [phase EXCEPT ![v] = "Quarantined"]
  /\ halted' = halted \cup {v}
  /\ UNCHANGED <<currentEpoch, minted, everMinted, credited, workerState,
                  workerValidator, workerEpoch, workerWasEligible,
                  historyDeleted, ineligibleCredit, unrecordedCredit>>

Burn(v) ==
  /\ phase[v] = "Quarantined"
  /\ phase' = [phase EXCEPT ![v] = "Burned"]
  /\ UNCHANGED <<currentEpoch, halted, minted, everMinted, credited,
                  workerState, workerValidator, workerEpoch,
                  workerWasEligible, historyDeleted, ineligibleCredit,
                  unrecordedCredit>>

AdvanceEpoch ==
  /\ currentEpoch < 2
  /\ currentEpoch' = currentEpoch + 1
  /\ UNCHANGED <<phase, halted, minted, everMinted, credited, workerState,
                  workerValidator, workerEpoch, workerWasEligible,
                  historyDeleted, ineligibleCredit, unrecordedCredit>>

BeginMint(w) ==
  LET v == Target[w] IN
  /\ workerState[w] = "Idle"
  /\ (Eligible(v, currentEpoch)
      \/ Defect = "SnapshotEligibility"
      \/ Defect = "IgnoreLedger"
      \/ Defect = "MintWhileHalted"
      \/ Defect = "UnrecordedCredit")
  /\ workerState' = [workerState EXCEPT ![w] = "Begun"]
  /\ workerValidator' = [workerValidator EXCEPT ![w] = v]
  /\ workerEpoch' = [workerEpoch EXCEPT ![w] = currentEpoch]
  /\ workerWasEligible' = [workerWasEligible EXCEPT ![w] = Eligible(v, currentEpoch)]
  /\ UNCHANGED <<currentEpoch, phase, halted, minted, everMinted,
                  credited, historyDeleted, ineligibleCredit,
                  unrecordedCredit>>

CommitMint(w) ==
  LET v == workerValidator[w]
      e == workerEpoch[w]
      eligibleNow == Eligible(v, e)
      shouldCredit ==
        IF Defect = "SnapshotEligibility"
        THEN workerWasEligible[w]
        ELSE IF Defect = "IgnoreLedger"
             THEN phase[v] = "Active" /\ v \notin halted
             ELSE IF Defect = "MintWhileHalted"
                  THEN MintKey(v, e) \notin minted
                  ELSE eligibleNow
  IN
  /\ workerState[w] = "Begun"
  /\ workerState' = [workerState EXCEPT ![w] = "Done"]
  /\ minted' =
       IF shouldCredit /\ Defect /= "UnrecordedCredit"
       THEN minted \cup {MintKey(v, e)}
       ELSE minted
  /\ everMinted' =
       IF shouldCredit
       THEN everMinted \cup {MintKey(v, e)}
       ELSE everMinted
  /\ credited' =
       IF shouldCredit
       THEN [credited EXCEPT ![v][e] = @ + 1]
       ELSE credited
  /\ ineligibleCredit' = (ineligibleCredit \/ (shouldCredit /\ ~eligibleNow))
  /\ unrecordedCredit' =
       (unrecordedCredit \/ (shouldCredit /\ Defect = "UnrecordedCredit"))
  /\ UNCHANGED <<currentEpoch, phase, halted, workerValidator,
                  workerEpoch, workerWasEligible, historyDeleted>>

Next ==
  \/ \E v \in Validators, replayEpoch \in Epochs : Redeem(v, replayEpoch)
  \/ \E v \in Validators : Slash(v)
  \/ \E v \in Validators : Burn(v)
  \/ AdvanceEpoch
  \/ \E w \in Workers : BeginMint(w)
  \/ \E w \in Workers : CommitMint(w)

Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ currentEpoch \in Epochs
  /\ phase \in [Validators -> Phases]
  /\ halted \subseteq Validators
  /\ minted \subseteq MintKeyType
  /\ everMinted \subseteq MintKeyType
  /\ credited \in [Validators -> [Epochs -> 0..3]]
  /\ workerState \in [Workers -> WorkerStates]
  /\ workerValidator \in [Workers -> Validators \cup {NoValidator}]
  /\ workerEpoch \in [Workers -> Epochs \cup {NoEpoch}]
  /\ workerWasEligible \in [Workers -> BOOLEAN]
  /\ historyDeleted \in BOOLEAN
  /\ ineligibleCredit \in BOOLEAN
  /\ unrecordedCredit \in BOOLEAN

MintLedgerIsAppendOnly ==
  /\ minted = everMinted
  /\ ~historyDeleted

AtMostOneCreditPerValidatorEpoch ==
  \A v \in Validators, e \in Epochs : credited[v][e] <= 1

EveryCreditHasPermanentEvidence ==
  \A v \in Validators, e \in Epochs :
    (credited[v][e] > 0) <=> (MintKey(v, e) \in everMinted)

MintRequiresCurrentEligibility == ~ineligibleCredit

MintAlwaysRecordsItsCredit == ~unrecordedCredit

HaltedValidatorsCannotMint ==
  \A v \in Validators : v \in halted => phase[v] /= "Active"

=============================================================================
