-------------------------- MODULE RetryBudgetRenewal --------------------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS HashCount, OperationCount, Persistent, MaxCycles, AllowUnknown, Unsafe
Hashes == 1..HashCount
Operations == 1..OperationCount
Sweepers == 1..2
Budget == 1

VARIABLES now, counts, stored, referenceStored, revision, deadline, provenance, rows, active,
          cycle, operations, sweeps, reference, freshRenewal, timelyRenewal,
          failureAtomic, dispatches, referenceDispatches, restarted, disk, healthy, uncertainUsed, uncertainOwner
vars == <<now, counts, stored, referenceStored, revision, deadline, provenance, rows, active,
          cycle, operations, sweeps, reference, freshRenewal, timelyRenewal,
          failureAtomic, dispatches, referenceDispatches, restarted, disk, healthy, uncertainUsed, uncertainOwner>>

Observed(os, h) == Cardinality({o \in Operations : os[o].hash = h /\ os[o].phase = "observed"})
Reserved(os, h) == Cardinality({o \in Operations : os[o].hash = h /\ os[o].phase = "reserved"})
Cancel(os, h) == [o \in Operations |->
    IF os[o].hash = h /\ os[o].phase \in {"reserved", "observed"}
    THEN [os[o] EXCEPT !.phase = "cancelled"] ELSE os[o]]
Persist(os, h) == [o \in Operations |->
    IF os[o].hash = h /\ os[o].phase = "observed"
    THEN [os[o] EXCEPT !.phase = "persisted"] ELSE os[o]]

Init ==
    /\ now = 0
    /\ operations \in [Operations -> [hash : Hashes, phase : (IF Persistent THEN {"idle", "reserved", "observed"} ELSE {"idle", "reserved"}), issued : {0}]]
    /\ counts = [h \in Hashes |-> Budget + Observed(operations, h)]
    /\ reference = counts
    /\ stored = IF Persistent THEN [h \in Hashes |-> Budget] ELSE counts
    /\ referenceStored = IF Persistent THEN [h \in Hashes |-> Budget] ELSE reference
    /\ revision = [h \in Hashes |-> 1]
    /\ deadline = [h \in Hashes |-> 1]
    /\ provenance = Hashes /\ rows = IF Persistent THEN Hashes ELSE {}
    /\ active = {} /\ cycle = [h \in Hashes |-> 0]
    /\ sweeps = [s \in Sweepers |-> [stage |-> "idle", time |-> 0, hash |-> 1,
                                      expected |-> 0, due |-> 0]]
    /\ freshRenewal = TRUE /\ timelyRenewal = TRUE /\ failureAtomic = TRUE
    /\ dispatches = 0 /\ referenceDispatches = 0
    /\ restarted = FALSE
    /\ disk = [revision |-> revision, deadline |-> deadline, cycle |-> cycle]
    /\ healthy = TRUE /\ uncertainUsed = FALSE /\ uncertainOwner = TRUE

Tick ==
    /\ now < MaxCycles + 1 /\ now' = now + 1
    /\ UNCHANGED <<counts, stored, referenceStored, revision, deadline, provenance, rows, active,
          cycle, operations, sweeps, reference, freshRenewal, timelyRenewal,
          failureAtomic, dispatches, referenceDispatches, restarted, disk, healthy, uncertainUsed, uncertainOwner>>

BeginSweep(s) ==
    /\ sweeps[s].stage \in {"idle", "done"}
    /\ \E h \in Hashes : deadline[h] # 0
    /\ sweeps' = [sweeps EXCEPT ![s].stage = "started", ![s].time = now]
    /\ UNCHANGED <<now, counts, stored, referenceStored, revision, deadline, provenance, rows, active,
          cycle, operations, reference, freshRenewal, timelyRenewal,
          failureAtomic, dispatches, referenceDispatches, restarted, disk, healthy, uncertainUsed, uncertainOwner>>

Prepare(s, h) ==
    /\ sweeps[s].stage = "started" /\ deadline[h] # 0
    /\ sweeps' = [sweeps EXCEPT ![s].stage = "prepared", ![s].hash = h,
                     ![s].expected = revision[h], ![s].due = deadline[h]]
    /\ UNCHANGED <<now, counts, stored, referenceStored, revision, deadline, provenance, rows, active,
          cycle, operations, reference, freshRenewal, timelyRenewal,
          failureAtomic, dispatches, referenceDispatches, restarted, disk, healthy, uncertainUsed, uncertainOwner>>

Renew(s) ==
    LET h == sweeps[s].hash
        fresh == IF Persistent THEN disk.revision[h] = sweeps[s].expected
                 ELSE deadline[h] = sweeps[s].due /\ deadline[h] # 0
        timely == sweeps[s].due # 0 /\ sweeps[s].due <= sweeps[s].time
        other == IF HashCount = 1 THEN h ELSE (h % HashCount) + 1
    IN
    /\ sweeps[s].stage = "prepared" /\ healthy
    /\ fresh \/ Unsafe = "stale"
    /\ timely \/ Unsafe = "early" \/ (Unsafe = "wall-clock" /\ sweeps[s].due <= now)
    /\ counts' = IF Unsafe = "other-hash"
                 THEN [counts EXCEPT ![h] = 0, ![other] = 0]
                 ELSE [counts EXCEPT ![h] = 0]
    /\ reference' = [reference EXCEPT ![h] = 0]
    /\ stored' = [stored EXCEPT ![h] = 0]
    /\ referenceStored' = [referenceStored EXCEPT ![h] = 0]
    /\ revision' = IF Persistent THEN [revision EXCEPT ![h] = sweeps[s].expected + 1] ELSE revision
    /\ deadline' = [deadline EXCEPT ![h] = 0]
    /\ provenance' = IF Unsafe = "provenance" THEN provenance \ {h} ELSE provenance
    /\ operations' = IF Unsafe = "tokens" THEN operations ELSE Cancel(operations, h)
    /\ cycle' = [cycle EXCEPT ![h] = @ + 1]
    /\ disk' = [revision |-> revision', deadline |-> deadline', cycle |-> cycle']
    /\ sweeps' = [sweeps EXCEPT ![s].stage = "done"]
    /\ freshRenewal' = (freshRenewal /\ fresh)
    /\ timelyRenewal' = (timelyRenewal /\ timely)
    /\ dispatches' = dispatches + (IF Unsafe = "probe" THEN 1 ELSE 0)
    /\ UNCHANGED <<now, rows, active, failureAtomic, referenceDispatches, restarted, healthy, uncertainUsed, uncertainOwner>>

FailedRenewal(s) ==
    LET h == sweeps[s].hash IN
    /\ Persistent /\ sweeps[s].stage = "prepared"
    /\ counts' = IF Unsafe = "partial-failure" THEN [counts EXCEPT ![h] = 0] ELSE counts
    /\ operations' = IF Unsafe = "failed-tokens" THEN Cancel(operations, h) ELSE operations
    /\ deadline' = IF Unsafe = "failed-metadata" THEN [deadline EXCEPT ![h] = 0] ELSE deadline
    /\ sweeps' = [sweeps EXCEPT ![s].stage = "done"]
    /\ UNCHANGED <<now, stored, referenceStored, revision, provenance, rows, active,
          cycle, reference, freshRenewal, timelyRenewal, dispatches, referenceDispatches, restarted, disk, healthy, uncertainUsed, uncertainOwner>>
    /\ failureAtomic' = (failureAtomic
       /\ <<counts', stored', referenceStored', revision', deadline', provenance', rows', cycle', active', operations'>>
        = <<counts, stored, referenceStored, revision, deadline, provenance, rows, cycle, active, operations>>)

Recite(h) ==
    /\ h \notin active /\ deadline[h] <= now
    /\ active' = active \cup {h}
    /\ dispatches' = dispatches + 1 /\ referenceDispatches' = referenceDispatches + 1
    /\ UNCHANGED <<now, counts, stored, referenceStored, revision, deadline, provenance, rows,
          cycle, operations, sweeps, reference, freshRenewal, timelyRenewal,
          failureAtomic, restarted, disk, healthy, uncertainUsed, uncertainOwner>>

Reserve(o, h) ==
    /\ h \in active /\ deadline[h] = 0
    /\ counts[h] + Reserved(operations, h) < Budget
    /\ operations[o].phase = "idle"
    /\ cycle[h] > 0
    /\ operations' = [operations EXCEPT ![o] = [hash |-> h, phase |-> "reserved", issued |-> cycle[h]]]
    /\ dispatches' = dispatches + 1 /\ referenceDispatches' = referenceDispatches + 1
    /\ UNCHANGED <<now, counts, stored, referenceStored, revision, deadline, provenance, rows, active,
          cycle, sweeps, reference, freshRenewal, timelyRenewal,
          failureAtomic, restarted, disk, healthy, uncertainUsed, uncertainOwner>>

Complete(o) ==
    LET h == operations[o].hash IN
    /\ operations[o].phase = "reserved"
        \/ (Unsafe = "cancelled-completion" /\ operations[o].phase = "cancelled"
             /\ operations[o].issued < cycle[h])
    /\ counts' = [counts EXCEPT ![h] = @ + 1]
    /\ reference' = IF operations[o].phase = "reserved" /\ operations[o].issued = cycle[h]
                     THEN [reference EXCEPT ![h] = @ + 1] ELSE reference
    /\ operations' = [operations EXCEPT ![o].phase = IF Persistent THEN "observed" ELSE "persisted"]
    /\ stored' = IF Persistent THEN stored ELSE [stored EXCEPT ![h] = counts'[h]]
    /\ referenceStored' = IF Persistent THEN referenceStored ELSE [referenceStored EXCEPT ![h] = reference'[h]]
    /\ UNCHANGED <<now, revision, deadline, provenance, rows, active,
          cycle, sweeps, freshRenewal, timelyRenewal, failureAtomic, dispatches, referenceDispatches, restarted, disk, healthy, uncertainUsed, uncertainOwner>>

Flush(h) ==
    /\ Persistent /\ healthy /\ Observed(operations, h) > 0
    /\ stored' = [stored EXCEPT ![h] = counts[h]]
    /\ referenceStored' = [referenceStored EXCEPT ![h] = reference[h]]
    /\ revision' = [revision EXCEPT ![h] = @ + 1]
    /\ operations' = Persist(operations, h)
    /\ disk' = [disk EXCEPT !.revision = revision']
    /\ UNCHANGED <<now, counts, deadline, provenance, rows, active,
          cycle, sweeps, reference, freshRenewal, timelyRenewal, failureAtomic, dispatches, referenceDispatches, restarted, healthy, uncertainUsed, uncertainOwner>>

Restart ==
    /\ Persistent /\ ~restarted
    /\ counts' = stored /\ reference' = referenceStored
    /\ revision' = disk.revision /\ deadline' = disk.deadline /\ cycle' = disk.cycle
    /\ healthy' = TRUE
    /\ operations' = [o \in Operations |-> IF operations[o].phase = "idle" THEN operations[o]
                      ELSE [operations[o] EXCEPT !.phase = "cancelled"]]
    /\ sweeps' = [s \in Sweepers |-> [stage |-> "idle", time |-> 0, hash |-> 1,
                                      expected |-> 0, due |-> 0]]
    /\ active' = {} /\ restarted' = TRUE
    /\ UNCHANGED <<now, stored, referenceStored, provenance, rows, disk, uncertainUsed, uncertainOwner,
          freshRenewal, timelyRenewal, failureAtomic, dispatches, referenceDispatches>>


UnconfirmedRenewal(s, published) ==
    LET h == sweeps[s].hash IN
    /\ AllowUnknown /\ Persistent /\ healthy /\ ~uncertainUsed
    /\ sweeps[s].stage = "prepared"
    /\ sweeps[s].due # 0 /\ sweeps[s].due <= sweeps[s].time
    /\ disk.revision[h] = sweeps[s].expected
    /\ stored' = IF published THEN [stored EXCEPT ![h] = 0] ELSE stored
    /\ referenceStored' = IF published THEN [referenceStored EXCEPT ![h] = 0] ELSE referenceStored
    /\ disk' = IF published THEN [disk EXCEPT !.revision[h] = @ + 1,
                                   !.deadline[h] = 0, !.cycle[h] = @ + 1] ELSE disk
    /\ counts' = IF Unsafe = "unknown-reset" THEN [counts EXCEPT ![h] = 0] ELSE counts
    /\ operations' = IF Unsafe = "unknown-tokens" THEN Cancel(operations, h) ELSE operations
    /\ healthy' = FALSE /\ uncertainUsed' = TRUE
    /\ sweeps' = [sweeps EXCEPT ![s].stage = "done"]
    /\ UNCHANGED <<now, revision, deadline, provenance, rows, active, cycle,
          reference, freshRenewal, timelyRenewal, failureAtomic, dispatches,
          referenceDispatches, restarted>>
    /\ uncertainOwner' = (uncertainOwner
          /\ <<counts', revision', deadline', provenance', rows', active', cycle', operations'>>
           = <<counts, revision, deadline, provenance, rows, active, cycle, operations>>)

Exhaust(h) ==
    /\ healthy /\ deadline[h] = 0 /\ counts[h] >= Budget /\ cycle[h] < MaxCycles
    /\ deadline' = [deadline EXCEPT ![h] = now + 1]
    /\ revision' = IF Persistent THEN [revision EXCEPT ![h] = @ + 1] ELSE revision
    /\ disk' = [disk EXCEPT !.deadline = deadline', !.revision = revision']
    /\ stored' = [stored EXCEPT ![h] = counts[h]]
    /\ referenceStored' = [referenceStored EXCEPT ![h] = reference[h]]
    /\ operations' = Persist(operations, h)
    /\ active' = active \ {h}
    /\ UNCHANGED <<now, counts, provenance, rows, cycle, sweeps, reference,
          freshRenewal, timelyRenewal, failureAtomic, dispatches, referenceDispatches,
          restarted, healthy, uncertainUsed, uncertainOwner>>

Next == Tick \/ Restart
     \/ (\E s \in Sweepers : BeginSweep(s) \/ Renew(s) \/ FailedRenewal(s)
                                \/ (\E h \in Hashes : Prepare(s, h))
                                \/ (\E published \in BOOLEAN : UnconfirmedRenewal(s, published)))
     \/ (\E h \in Hashes : Recite(h) \/ Flush(h) \/ Exhaust(h))
     \/ (\E o \in Operations : Complete(o) \/ (\E h \in Hashes : Reserve(o, h)))

Inv_UnconfirmedOwner == uncertainOwner
Inv_FreshRenewal == freshRenewal
Inv_SweepTime == timelyRenewal
Inv_AtomicFailure == failureAtomic
Inv_NoAutonomousProbe == dispatches = referenceDispatches
Inv_Provenance == provenance = Hashes /\ rows = (IF Persistent THEN Hashes ELSE {})
Inv_CycleAccounting == counts = reference
Inv_CurrentTokens == \A o \in Operations :
    operations[o].phase \in {"reserved", "observed"} => operations[o].issued = cycle[operations[o].hash]
Inv_DurableAccounting == stored = referenceStored
Inv_StoredBound == \A h \in Hashes : stored[h] <= counts[h]
Spec == Init /\ [][Next]_vars
=============================================================================
