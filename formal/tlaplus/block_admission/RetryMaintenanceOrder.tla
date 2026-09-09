------------------------- MODULE RetryMaintenanceOrder -------------------------
EXTENDS Integers, FiniteSets
CONSTANTS Hashes, Unsafe
Budget == 1
VARIABLES now, active, generation, attempts, deadline, lastRequest, createdAt,
          phase, passTime, remaining, snapshot, delayedStamp, observed, observedDue,
          orderOK, decisionOK, snapshotOK, clockOK, currentDueOK
vars == <<now, active, generation, attempts, deadline, lastRequest, createdAt,
          phase, passTime, remaining, snapshot, delayedStamp, observed, observedDue,
          orderOK, decisionOK, snapshotOK, clockOK, currentDueOK>>
Policy(h) == [generation |-> generation[h], attempts |-> attempts[h],
              deadline |-> deadline[h], timestamp |-> lastRequest[h]]
Decision(p, time) == IF p.deadline > time \/ time <= p.timestamp THEN "skip"
                    ELSE IF p.attempts >= Budget THEN "retire" ELSE "dispatch"
Selection(p, time) == IF p.deadline > time THEN "skip"
                     ELSE IF p.attempts >= Budget THEN "retire" ELSE "dispatch"

Init == /\ now = 0 /\ active \in SUBSET Hashes
        /\ generation = [h \in Hashes |-> 1]
        /\ attempts \in [Hashes -> 0..Budget]
        /\ deadline = [h \in Hashes |-> IF h \in active THEN 0 ELSE 1]
        /\ lastRequest = [h \in Hashes |-> 0] /\ createdAt = lastRequest
        /\ phase = "idle" /\ passTime = 0 /\ remaining = {}
        /\ snapshot = [h \in Hashes |-> Policy(h)] /\ delayedStamp = {}
        /\ observed = {} /\ observedDue = [h \in Hashes |-> FALSE]
        /\ currentDueOK = TRUE
        /\ orderOK = TRUE /\ decisionOK = TRUE /\ snapshotOK = TRUE /\ clockOK = TRUE

Tick == /\ now < 2 /\ now' = now + 1
        /\ UNCHANGED <<active, generation, attempts, deadline, lastRequest, createdAt,
              phase, passTime, remaining, snapshot, delayedStamp, observed, observedDue,
              orderOK, decisionOK, snapshotOK, clockOK, currentDueOK>>

Publish(h) == /\ h \in active /\ active' = active \ {h}
              /\ UNCHANGED <<now, generation, attempts, deadline, lastRequest, createdAt,
                    phase, passTime, remaining, snapshot, delayedStamp, observed, observedDue,
                    orderOK, decisionOK, snapshotOK, clockOK, currentDueOK>>

Recite(h) == /\ h \notin active /\ deadline[h] <= now
             /\ active' = active \cup {h}
             /\ \E nextGeneration \in generation[h]..2 :
                    generation' = [generation EXCEPT ![h] = nextGeneration]
             /\ createdAt' = [createdAt EXCEPT ![h] = now]
             /\ lastRequest' = [lastRequest EXCEPT ![h] =
                    IF Unsafe \in {"old-clock", "late-clock"} THEN @ ELSE now]
             /\ delayedStamp' = IF Unsafe = "late-clock" THEN delayedStamp \cup {h} ELSE delayedStamp
             /\ clockOK' = (clockOK /\ lastRequest'[h] = createdAt'[h])
             /\ UNCHANGED <<now, attempts, deadline, phase, passTime, remaining, snapshot,
                    observed, observedDue, orderOK, decisionOK, snapshotOK, currentDueOK>>

Stamp(h) == /\ h \in delayedStamp /\ lastRequest' = [lastRequest EXCEPT ![h] = createdAt[h]]
            /\ delayedStamp' = delayedStamp \ {h}
            /\ UNCHANGED <<now, active, generation, attempts, deadline, createdAt,
                  phase, passTime, remaining, snapshot, observed, observedDue,
                  orderOK, decisionOK, snapshotOK, clockOK, currentDueOK>>

Begin == /\ phase = "idle" /\ phase' = "examining"
         /\ passTime' = now /\ remaining' = active
         /\ snapshot' = [h \in Hashes |-> Policy(h)]
         /\ UNCHANGED <<now, active, generation, attempts, deadline, lastRequest, createdAt,
               delayedStamp, observed, observedDue, orderOK, decisionOK, snapshotOK, clockOK, currentDueOK>>

Observe(h) ==
    /\ phase = "examining" /\ h \in remaining \ observed
    /\ observed' = observed \cup {h}
    /\ observedDue' = [observedDue EXCEPT ![h] = Decision(Policy(h), passTime) # "skip"]
    /\ UNCHANGED <<now, active, generation, attempts, deadline, lastRequest, createdAt,
          phase, passTime, remaining, snapshot, delayedStamp,
          orderOK, decisionOK, snapshotOK, clockOK, currentDueOK>>

Apply(h) ==
    /\ phase = "examining" /\ h \in remaining \cap observed
    /\ LET current == h \in active /\ generation[h] = snapshot[h].generation
           expected == IF current /\ observedDue[h] THEN Selection(Policy(h), now) ELSE "skip"
           candidate == IF Unsafe = "stale-snapshot" /\ ~current THEN snapshot[h]
                        ELSE IF Unsafe = "reset-before-decision" /\ deadline[h] > 0 /\ deadline[h] <= passTime
                             THEN [Policy(h) EXCEPT !.attempts = 0] ELSE Policy(h)
           selected == IF Unsafe = "stale-snapshot" /\ ~current /\ h \in active
                       THEN Decision(candidate, passTime)
                       ELSE IF current /\ observedDue[h] THEN Selection(candidate, now) ELSE "skip"
       IN
       /\ active' = IF selected = "retire" THEN active \ {h} ELSE active
       /\ attempts' = IF selected = "dispatch" THEN [attempts EXCEPT ![h] = candidate.attempts + 1] ELSE attempts
       /\ deadline' = IF selected = "retire" THEN [deadline EXCEPT ![h] = now + 1]
                       ELSE IF selected = "dispatch" THEN [deadline EXCEPT ![h] = 0] ELSE deadline
       /\ lastRequest' = IF selected = "dispatch" THEN [lastRequest EXCEPT ![h] = now] ELSE lastRequest
       /\ decisionOK' = (decisionOK /\ selected = expected)
       /\ snapshotOK' = (snapshotOK /\ (selected = "skip" \/ current))
       /\ currentDueOK' = (currentDueOK /\ (selected = "skip" \/ Decision(Policy(h), now) # "skip"))
    /\ remaining' = remaining \ {h}
    /\ observed' = observed \ {h}
    /\ UNCHANGED <<now, generation, createdAt, phase, passTime, snapshot, delayedStamp, orderOK, clockOK>>
    /\ UNCHANGED observedDue

Sweep ==
    /\ phase = "examining" /\ (remaining = {} \/ Unsafe = "early-expiry")
    /\ attempts' = [h \in Hashes |-> IF deadline[h] > 0 /\ deadline[h] <= passTime THEN 0 ELSE attempts[h]]
    /\ deadline' = [h \in Hashes |-> IF deadline[h] > 0 /\ deadline[h] <= passTime THEN 0 ELSE deadline[h]]
    /\ phase' = "done" /\ orderOK' = (orderOK /\ remaining = {} /\ observed = {})
    /\ UNCHANGED <<now, active, generation, lastRequest, createdAt, passTime, remaining, snapshot,
          delayedStamp, observed, observedDue, decisionOK, snapshotOK, clockOK, currentDueOK>>

Next == Tick \/ Begin \/ Sweep \/ (\E h \in Hashes : Publish(h) \/ Recite(h) \/ Stamp(h) \/ Observe(h) \/ Apply(h))
Inv_Order == orderOK
Inv_Snapshot == snapshotOK
Inv_Decision == decisionOK
Inv_Clock == clockOK
Inv_CurrentDue == currentDueOK
TypeOK == /\ now \in 0..2 /\ active \subseteq Hashes /\ generation \in [Hashes -> 1..2]
          /\ attempts \in [Hashes -> 0..1] /\ deadline \in [Hashes -> 0..3]
          /\ lastRequest \in [Hashes -> 0..2] /\ createdAt \in [Hashes -> 0..2]
          /\ phase \in {"idle", "examining", "done"} /\ passTime \in 0..2
          /\ remaining \subseteq Hashes /\ delayedStamp \subseteq Hashes
          /\ observed \subseteq Hashes /\ observedDue \in [Hashes -> BOOLEAN]
          /\ currentDueOK \in BOOLEAN
          /\ orderOK \in BOOLEAN /\ decisionOK \in BOOLEAN /\ snapshotOK \in BOOLEAN /\ clockOK \in BOOLEAN
Spec == Init /\ [][Next]_vars
=============================================================================
