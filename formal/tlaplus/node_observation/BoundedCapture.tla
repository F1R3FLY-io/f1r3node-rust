------------------------- MODULE BoundedCapture -------------------------
EXTENDS Naturals, FiniteSets
CONSTANT Bug
Envs == {"dag", "blocks"}
VARIABLES phase, guards, transactions, before, opened, validated, generation,
          startGeneration, endGeneration, env, copied, complete, limitsOK,
          backendOK, finiteDeadline, contended, expired, lateAcquisition, accepted, observerWrites
vars == <<phase, guards, transactions, before, opened, validated, generation,
          startGeneration, endGeneration, env, copied, complete, limitsOK,
          backendOK, finiteDeadline, contended, expired, lateAcquisition, accepted, observerWrites>>
Init == /\ phase = "new" /\ guards = {} /\ transactions = {}
        /\ before = [e \in Envs |-> 0] /\ opened = before /\ validated = before
        /\ generation = 0 /\ startGeneration = 0 /\ endGeneration = 0
        /\ env = [e \in Envs |-> 0] /\ copied = 0
        /\ complete \in BOOLEAN /\ limitsOK \in BOOLEAN /\ backendOK \in BOOLEAN
        /\ finiteDeadline \in BOOLEAN /\ contended \in BOOLEAN /\ expired \in BOOLEAN
        /\ lateAcquisition = FALSE /\ accepted = FALSE /\ observerWrites = 0
Keep == UNCHANGED <<complete, limitsOK, backendOK, finiteDeadline, contended>>
Reject == /\ phase \notin {"done", "rejected"}
          /\ phase' = "rejected"
          /\ guards' = (IF Bug = "release" THEN guards ELSE {})
          /\ transactions' = {}
          /\ UNCHANGED <<before, opened, validated, generation, startGeneration,
                         endGeneration, env, copied, expired, lateAcquisition, accepted, observerWrites>>
          /\ Keep
Admit == /\ phase = "new"
         /\ ((limitsOK /\ finiteDeadline) \/ Bug = "admission")
         /\ phase' = "lock1"
         /\ UNCHANGED <<guards, transactions, before, opened, validated, generation,
                        startGeneration, endGeneration, env, copied, expired, lateAcquisition,
                        accepted, observerWrites>> /\ Keep
Lock(n) == /\ (phase = (CASE n = 1 -> "lock1" [] n = 2 -> "lock2" [] OTHER -> "lock3") \/ (Bug = "order" /\ phase = "lock1" /\ n = 3))
           /\ (~(expired /\ contended) \/ Bug = "deadline")
           /\ guards' = guards \cup {n}
           /\ lateAcquisition' = (lateAcquisition \/ (expired /\ contended))
           /\ phase' = CASE n = 1 -> "lock2" [] n = 2 -> "lock3" [] OTHER -> "state"
           /\ UNCHANGED <<transactions, before, opened, validated, generation,
                          startGeneration, endGeneration, env, copied, expired,
                          accepted, observerWrites>> /\ Keep
CopyState == /\ phase = "state" /\ phase' = "beforeDag"
             /\ startGeneration' = generation /\ guards' = {1, 2}
             /\ UNCHANGED <<transactions, before, opened, validated, generation,
                            endGeneration, env, copied, expired, lateAcquisition, accepted, observerWrites>> /\ Keep
ReadBefore(e, p, next) == /\ phase = p /\ phase' = next
    /\ before' = [before EXCEPT ![e] = env[e]]
    /\ UNCHANGED <<guards, transactions, opened, validated, generation,
                   startGeneration, endGeneration, env, copied, expired, lateAcquisition, accepted, observerWrites>> /\ Keep
Open(e, p, next) == /\ phase = p /\ phase' = next
    /\ (backendOK \/ Bug = "admission")
    /\ (before[e] = env[e] \/ Bug = "open")
    /\ opened' = [opened EXCEPT ![e] = env[e]] /\ transactions' = transactions \cup {e}
    /\ UNCHANGED <<guards, before, validated, generation, startGeneration,
                   endGeneration, env, copied, expired, lateAcquisition, accepted, observerWrites>> /\ Keep
Rows(size) == /\ phase = "rows" /\ size \in {1, 2, 3}
    /\ (size <= 2 \/ Bug = "bytes") /\ (complete \/ Bug = "incomplete")
    /\ copied' = size /\ phase' = "validateDag"
    /\ UNCHANGED <<guards, transactions, before, opened, validated, generation,
                   startGeneration, endGeneration, env, expired, lateAcquisition, accepted, observerWrites>> /\ Keep
Validate(e, p, next) == /\ phase = p /\ phase' = next
    /\ (opened[e] = env[e] \/ Bug = "validation")
    /\ validated' = [validated EXCEPT ![e] = env[e]]
    /\ UNCHANGED <<guards, transactions, before, opened, generation, startGeneration,
                   endGeneration, env, copied, expired, lateAcquisition, accepted, observerWrites>> /\ Keep
Release == /\ phase = "generation" /\ phase' = "serialize"
    /\ (startGeneration = generation \/ Bug = "generation")
    /\ endGeneration' = generation /\ transactions' = {}
    /\ guards' = (IF Bug = "release" THEN guards ELSE {})
    /\ UNCHANGED <<before, opened, validated, generation, startGeneration,
                   env, copied, expired, lateAcquisition, accepted, observerWrites>> /\ Keep
Serialize == /\ phase = "serialize" /\ phase' = "done" /\ accepted' = TRUE
    /\ observerWrites' = (IF Bug = "write" THEN 1 ELSE 0)
    /\ UNCHANGED <<guards, transactions, before, opened, validated, generation,
                   startGeneration, endGeneration, env, copied, expired, lateAcquisition>> /\ Keep
Writer(e) == /\ phase \notin {"new", "done", "rejected"} /\ env[e] < 2
    /\ env' = [env EXCEPT ![e] = @ + 1]
    /\ UNCHANGED <<phase, guards, transactions, before, opened, validated, generation,
                   startGeneration, endGeneration, copied, expired, lateAcquisition, accepted, observerWrites>> /\ Keep
Insert == /\ phase \notin {"new", "done", "rejected"} /\ generation < 1
    /\ generation' = generation + 1
    /\ UNCHANGED <<phase, guards, transactions, before, opened, validated,
                   startGeneration, endGeneration, env, copied, expired, lateAcquisition, accepted, observerWrites>> /\ Keep
Expire == /\ phase \in {"lock1", "lock2", "lock3"} /\ ~expired /\ expired' = TRUE
          /\ UNCHANGED <<phase, guards, transactions, before, opened, validated,
                         generation, startGeneration, endGeneration, env, copied,
                         lateAcquisition, accepted, observerWrites>> /\ Keep
Next == Expire \/ Admit \/ (\E n \in 1..3: Lock(n)) \/ CopyState
        \/ ReadBefore("dag", "beforeDag", "openDag")
        \/ Open("dag", "openDag", "beforeBlocks")
        \/ ReadBefore("blocks", "beforeBlocks", "openBlocks")
        \/ Open("blocks", "openBlocks", "rows")
        \/ (\E size \in {1, 2, 3}: Rows(size))
        \/ Validate("dag", "validateDag", "validateBlocks")
        \/ Validate("blocks", "validateBlocks", "generation")
        \/ Release \/ Serialize \/ Reject \/ (\E e \in Envs: Writer(e)) \/ Insert
Spec == Init /\ [][Next]_vars
ValidAdmission == /\ (phase \notin {"new", "rejected"} => limitsOK /\ finiteDeadline)
                  /\ (accepted => backendOK)
BoundLockWait == ~lateAcquisition
GuardOrder == /\ (2 \in guards => 1 \in guards) /\ (3 \in guards => {1, 2} \subseteq guards)
OpenIdentity == \A e \in transactions: before[e] = opened[e]
ValidatedIdentity == accepted => \A e \in Envs: before[e] = opened[e] /\ opened[e] = validated[e]
GenerationStable == accepted => startGeneration = endGeneration
CompleteRows == accepted => complete
BoundBytes == copied <= 2
Detached == phase \in {"serialize", "done", "rejected"} => guards = {} /\ transactions = {}
ReadOnly == observerWrites = 0
=============================================================================
