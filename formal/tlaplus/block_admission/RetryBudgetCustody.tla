------------------------- MODULE RetryBudgetCustody -------------------------
EXTENDS Integers, FiniteSets
CONSTANTS Hashes, Unsafe
Budget == 2
Capacity == 1
VARIABLES slots, reference, deadline, active, provenance, required, schedule, now,
          activationAllowed, expectedDeadline, dispatches, authorizedDispatches, failurePreserved
vars == <<slots, reference, deadline, active, provenance, required, schedule, now,
          activationAllowed, expectedDeadline, dispatches, authorizedDispatches, failurePreserved>>
Present(s) == {k \in {"volatile", "pending", "retired"} : s[k] >= 0}
Total(s) == (IF s.volatile >= 0 THEN s.volatile ELSE 0)
          + (IF s.pending >= 0 THEN s.pending ELSE 0)
          + (IF s.retired >= 0 THEN s.retired ELSE 0)

Init == /\ active \in SUBSET Hashes /\ Cardinality(active) = Capacity
        /\ slots = [h \in Hashes |-> [volatile |-> IF h \in active THEN Budget ELSE -1,
                                      pending |-> -1, retired |-> -1]]
        /\ reference = [h \in Hashes |-> IF h \in active THEN Budget ELSE 0]
        /\ deadline = [h \in Hashes |-> 0]
        /\ provenance = [h \in Hashes |-> h \in active]
        /\ required = provenance
        /\ schedule = [h \in Hashes |-> IF h \in active THEN 1 ELSE 0]
        /\ now = 0 /\ activationAllowed = TRUE
        /\ expectedDeadline = deadline
        /\ dispatches = [h \in Hashes |-> 0] /\ authorizedDispatches = dispatches
        /\ failurePreserved = TRUE

Retire(h) ==
    /\ h \in active /\ Total(slots[h]) >= Budget
    /\ slots' = [slots EXCEPT ![h] =
          IF slots[h].pending >= 0 THEN slots[h]
          ELSE [volatile |-> IF Unsafe = "duplicate-budget" THEN slots[h].volatile ELSE -1,
                pending |-> -1, retired |-> Total(slots[h])]]
    /\ deadline' = [deadline EXCEPT ![h] = now + 1]
    /\ expectedDeadline' = [expectedDeadline EXCEPT ![h] = now + 1]
    /\ active' = active \ {h}
    /\ provenance' = [provenance EXCEPT ![h] =
          IF slots[h].pending >= 0 THEN (IF Unsafe = "pending-provenance" THEN FALSE ELSE @)
          ELSE IF Unsafe = "retired-provenance" THEN @ ELSE FALSE]
    /\ schedule' = [schedule EXCEPT ![h] = IF Unsafe = "retired-schedule" THEN @ ELSE 0]
    /\ UNCHANGED <<reference, required, now, activationAllowed, dispatches, authorizedDispatches, failurePreserved>>

Recite(h, dependency) ==
    /\ h \notin active /\ Cardinality(active) < Capacity
    /\ deadline[h] <= now \/ Unsafe = "early-activation"
    /\ slots' = [slots EXCEPT ![h] =
          IF slots[h].pending >= 0 THEN slots[h]
          ELSE [volatile |-> Total(slots[h]), pending |-> -1, retired |-> -1]]
    /\ active' = active \cup {h}
    /\ provenance' = [provenance EXCEPT ![h] = @ \/ dependency]
    /\ schedule' = [schedule EXCEPT ![h] = 1]
    /\ activationAllowed' = (activationAllowed /\ deadline[h] <= now)
    /\ UNCHANGED <<reference, deadline, required, now, expectedDeadline, dispatches, authorizedDispatches, failurePreserved>>

Publish(h, captured) ==
    /\ slots' = [slots EXCEPT ![h] = [volatile |-> -1, pending |-> Total(slots[h]), retired |-> -1]]
    /\ active' = active \ {h}
    /\ provenance' = [provenance EXCEPT ![h] =
          IF Unsafe = "overwrite-provenance" THEN captured ELSE @ \/ captured]
    /\ required' = [required EXCEPT ![h] = provenance[h] \/ captured]
    /\ deadline' = IF Unsafe = "lost-deadline" THEN [deadline EXCEPT ![h] = 0] ELSE deadline
    /\ UNCHANGED <<reference, schedule, now, activationAllowed, expectedDeadline, dispatches, authorizedDispatches, failurePreserved>>

FailedPublish(h) ==
    /\ slots' = IF Unsafe = "failed-transfer"
                 THEN [slots EXCEPT ![h].retired = -1] ELSE slots
    /\ provenance' = IF Unsafe = "failure-provenance" THEN [provenance EXCEPT ![h] = FALSE] ELSE provenance
    /\ UNCHANGED <<reference, deadline, active, required, schedule, now,
          activationAllowed, expectedDeadline, dispatches, authorizedDispatches>>
    /\ failurePreserved' = (failurePreserved /\
          <<slots', deadline', active', provenance', schedule'>> = <<slots, deadline, active, provenance, schedule>>)

CapacityRefusal(h) ==
    /\ h \notin active /\ Cardinality(active) = Capacity
    /\ slots' = IF Unsafe = "capacity-loss"
                 THEN [slots EXCEPT ![h].retired = -1] ELSE slots
    /\ UNCHANGED <<reference, deadline, active, provenance, required, schedule, now,
          activationAllowed, expectedDeadline, dispatches, authorizedDispatches, failurePreserved>>

CompleteRetry(h) ==
    /\ h \in active /\ deadline[h] = 0 /\ Total(slots[h]) < Budget /\ dispatches[h] < 4
    /\ slots' = [slots EXCEPT ![h] =
          IF slots[h].pending >= 0 THEN [slots[h] EXCEPT !.pending = @ + 1]
          ELSE [slots[h] EXCEPT !.volatile = @ + 1]]
    /\ reference' = [reference EXCEPT ![h] = @ + 1]
    /\ dispatches' = [dispatches EXCEPT ![h] = @ + 1]
    /\ authorizedDispatches' = [authorizedDispatches EXCEPT ![h] = @ + 1]
    /\ UNCHANGED <<deadline, active, provenance, required, schedule, now,
          activationAllowed, expectedDeadline, failurePreserved>>

Expire(h) ==
    /\ deadline[h] > 0 /\ deadline[h] <= now
    /\ slots' = [slots EXCEPT ![h] =
          [volatile |-> IF slots[h].volatile >= 0 THEN 0 ELSE -1,
           pending |-> IF slots[h].pending >= 0 THEN 0 ELSE -1, retired |-> -1]]
    /\ reference' = [reference EXCEPT ![h] = 0]
    /\ deadline' = [deadline EXCEPT ![h] = 0]
    /\ expectedDeadline' = [expectedDeadline EXCEPT ![h] = 0]
    /\ dispatches' = IF Unsafe = "expiry-probe" THEN [dispatches EXCEPT ![h] = @ + 1] ELSE dispatches
    /\ schedule' = IF Unsafe = "expiry-schedule" THEN [schedule EXCEPT ![h] = 0] ELSE schedule
    /\ UNCHANGED <<active, provenance, required, now, activationAllowed, authorizedDispatches, failurePreserved>>

Tick == /\ now < 2 /\ now' = now + 1
        /\ UNCHANGED <<slots, reference, deadline, active, provenance, required, schedule,
              activationAllowed, expectedDeadline, dispatches, authorizedDispatches, failurePreserved>>

Next == Tick \/ (\E h \in Hashes : Retire(h) \/ FailedPublish(h) \/ Expire(h)
                   \/ CapacityRefusal(h) \/ CompleteRetry(h)
                   \/ (\E dependency \in BOOLEAN : Recite(h, dependency))
                   \/ (\E captured \in BOOLEAN : Publish(h, captured)))
TypeOK == /\ slots \in [Hashes -> [volatile : -1..Budget, pending : -1..Budget, retired : -1..Budget]]
          /\ reference \in [Hashes -> 0..Budget] /\ deadline \in [Hashes -> 0..3]
          /\ active \subseteq Hashes /\ Cardinality(active) <= Capacity
          /\ provenance \in [Hashes -> BOOLEAN] /\ required \in [Hashes -> BOOLEAN]
          /\ schedule \in [Hashes -> 0..1] /\ now \in 0..2
          /\ activationAllowed \in BOOLEAN /\ failurePreserved \in BOOLEAN
          /\ expectedDeadline \in [Hashes -> 0..3]
          /\ dispatches \in [Hashes -> 0..5] /\ authorizedDispatches \in [Hashes -> 0..4]
Inv_SingleBudget == \A h \in Hashes : Cardinality(Present(slots[h])) <= 1
Inv_Budget == \A h \in Hashes : Total(slots[h]) = reference[h]
Inv_Retired == \A h \in Hashes : slots[h].retired >= 0 =>
                   ~provenance[h] /\ schedule[h] = 0 /\ h \notin active
Inv_PendingProvenance == \A h \in Hashes : slots[h].pending >= 0 /\ required[h] => provenance[h]
Inv_ActiveSchedule == \A h \in active : schedule[h] = 1
Inv_Activation == activationAllowed
Inv_NoProbe == dispatches = authorizedDispatches
Inv_Deadline == deadline = expectedDeadline
Inv_Failure == failurePreserved
Inv_Ownership == /\ \A h \in active : slots[h].volatile >= 0 \/ slots[h].pending >= 0
                 /\ \A h \in Hashes : slots[h].retired >= 0 => deadline[h] > 0
Spec == Init /\ [][Next]_vars
=============================================================================
