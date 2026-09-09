----------------------- MODULE FinalizationLedgerAudit -----------------------
EXTENDS Integers, FiniteSets

CONSTANTS
    \* @type: Int;
    MaxRounds,
    \* @type: Int;
    MaxReaders,
    \* @type: Int;
    MaxCrashes,
    \* @type: Int;
    PageLimit,
    \* @type: Str;
    Bug

ASSUME /\ MaxRounds > 0
       /\ MaxReaders > 0
       /\ MaxCrashes >= 0
       /\ PageLimit > 0
       /\ Bug \in {"none", "skip", "early-ready", "saved-cursor",
                     "moving-target", "over-budget"}

Readers == 1..MaxReaders
Rounds == 1..MaxRounds
Prefix(revision) == {round \in Rounds : round <= revision}

VARIABLES
    \* @type: Int;
    head,
    \* @type: Set(Int);
    corrupt,
    \* @type: Int -> Str;
    status,
    \* @type: Int -> Int;
    target,
    \* @type: Int -> Int;
    captured,
    \* @type: Int -> Int;
    cursor,
    \* @type: Int -> Set(Int);
    examined,
    \* @type: Int -> Int;
    lastWork,
    \* @type: Int -> Int;
    crashes

vars == <<head, corrupt, status, target, captured, cursor, examined,
          lastWork, crashes>>

Init ==
    /\ head \in 0..MaxRounds
    /\ corrupt \in SUBSET Rounds
    /\ corrupt \subseteq Prefix(head)
    /\ status = [r \in Readers |-> "idle"]
    /\ target = [r \in Readers |-> 0]
    /\ captured = [r \in Readers |-> 0]
    /\ cursor = [r \in Readers |-> 0]
    /\ examined = [r \in Readers |-> {}]
    /\ lastWork = [r \in Readers |-> 0]
    /\ crashes = [r \in Readers |-> 0]

Start(r) ==
    /\ status[r] = "idle"
    /\ status' = [status EXCEPT ![r] = "audit"]
    /\ target' = [target EXCEPT ![r] = head]
    /\ captured' = [captured EXCEPT ![r] = head]
    /\ cursor' = [cursor EXCEPT ![r] =
                     IF Bug = "saved-cursor" /\ crashes[r] > 0 THEN head ELSE 0]
    /\ examined' = [examined EXCEPT ![r] = {}]
    /\ lastWork' = [lastWork EXCEPT ![r] = 0]
    /\ UNCHANGED <<head, corrupt, crashes>>

AuditBatch(r, n) ==
    /\ status[r] = "audit"
    /\ n \in Rounds
    /\ n <= target[r] - cursor[r]
    /\ Bug = "over-budget" \/ n <= PageLimit
    /\ LET page == {round \in Rounds : cursor[r] < round /\ round <= cursor[r] + n}
           checked == IF Bug = "skip" THEN page \ {cursor[r] + 1} ELSE page
           valid == checked \cap corrupt = {}
       IN /\ lastWork' = [lastWork EXCEPT ![r] = n]
          /\ status' = [status EXCEPT ![r] = IF valid THEN "audit" ELSE "failed"]
          /\ cursor' = [cursor EXCEPT ![r] = IF valid THEN @ + n ELSE @]
          /\ examined' = [examined EXCEPT ![r] = IF valid THEN @ \cup checked ELSE @]
    /\ UNCHANGED <<head, corrupt, target, captured, crashes>>

Audit(r) == \E n \in 1..MaxRounds : AuditBatch(r, n)

Finish(r) ==
    /\ status[r] = "audit"
    /\ cursor[r] = target[r] \/ Bug = "early-ready"
    /\ status' = [status EXCEPT ![r] = "ready"]
    /\ UNCHANGED <<head, corrupt, target, captured, cursor, examined,
                    lastWork, crashes>>

AppendRound ==
    /\ head < MaxRounds
    /\ head' = head + 1
    /\ target' = [r \in Readers |->
          IF Bug = "moving-target" /\ status[r] = "audit"
          THEN head + 1 ELSE target[r]]
    /\ UNCHANGED <<corrupt, status, captured, cursor, examined, lastWork, crashes>>

Crash(r) ==
    /\ status[r] # "idle"
    /\ crashes[r] < MaxCrashes
    /\ status' = [status EXCEPT ![r] = "idle"]
    /\ cursor' = [cursor EXCEPT ![r] = 0]
    /\ examined' = [examined EXCEPT ![r] = {}]
    /\ lastWork' = [lastWork EXCEPT ![r] = 0]
    /\ crashes' = [crashes EXCEPT ![r] = @ + 1]
    /\ UNCHANGED <<head, corrupt, target, captured>>

Next ==
    \/ \E r \in Readers : Start(r)
    \/ \E r \in Readers : Audit(r)
    \/ \E r \in Readers : Finish(r)
    \/ AppendRound
    \/ \E r \in Readers : Crash(r)

Spec == Init /\ [][Next]_vars
FairSpec == Spec /\ \A r \in Readers :
    WF_vars(Start(r)) /\ WF_vars(Audit(r)) /\ WF_vars(Finish(r))

TypeOK ==
    /\ head \in 0..MaxRounds
    /\ corrupt \in SUBSET Rounds
    /\ corrupt \subseteq Prefix(head)
    /\ status \in [Readers -> {"idle", "audit", "ready", "failed"}]
    /\ target \in [Readers -> 0..MaxRounds]
    /\ captured \in [Readers -> 0..MaxRounds]
    /\ cursor \in [Readers -> 0..MaxRounds]
    /\ examined \in [Readers -> SUBSET Rounds]
    /\ lastWork \in [Readers -> 0..MaxRounds]
    /\ crashes \in [Readers -> 0..MaxCrashes]

Inv_ExactTarget == \A r \in Readers : target[r] = captured[r]
Inv_CursorBounds == \A r \in Readers : cursor[r] <= target[r] /\ target[r] <= head
Inv_ValidatedPrefix == \A r \in Readers :
    examined[r] = Prefix(cursor[r]) /\ examined[r] \cap corrupt = {}
Inv_Readiness == \A r \in Readers : status[r] = "ready" =>
    examined[r] = Prefix(captured[r]) /\ examined[r] \cap corrupt = {}
Inv_BoundedBatch == \A r \in Readers : lastWork[r] <= PageLimit
Inv_RestartClearsProgress == \A r \in Readers : status[r] = "idle" =>
    cursor[r] = 0 /\ examined[r] = {}

Safety == TypeOK /\ Inv_ExactTarget /\ Inv_CursorBounds /\ Inv_ValidatedPrefix
          /\ Inv_Readiness /\ Inv_BoundedBatch /\ Inv_RestartClearsProgress

AuditEventuallyTerminates == \A r \in Readers : <>[](status[r] \in {"ready", "failed"})
=============================================================================
