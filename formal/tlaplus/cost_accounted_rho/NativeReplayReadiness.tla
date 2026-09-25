-------------------- MODULE NativeReplayReadiness --------------------
EXTENDS Naturals, FiniteSets, TLC
CONSTANTS HoldWaitingLease, SkipRecheck, SkipDirectCheck
VARIABLES stage, done, writer, restored
vars == <<stage, done, writer, restored>>
Slots == {1, 2, 3}
Predecessors(s) == IF s = 2 THEN {1} ELSE {}
Ready(s) == Predecessors(s) \subseteq done
Leases == {s \in Slots : stage[s] \in {"Leased", "Running"}
  \/ (HoldWaitingLease /\ stage[s] = "Waiting")}

Init == /\ stage = [s \in Slots |-> "Idle"]
        /\ done = {}
        /\ writer = "Idle"
        /\ restored = FALSE

Request(s) == /\ stage[s] = "Idle"
              /\ stage' = [stage EXCEPT ![s] = "Waiting"]
              /\ UNCHANGED <<done, writer, restored>>
DirectReserve(s) == /\ stage[s] = "Idle"
                    /\ (SkipDirectCheck \/ Ready(s))
                    /\ stage' = [stage EXCEPT ![s] = "Running"]
                    /\ UNCHANGED <<done, writer, restored>>
Awake(s) == /\ stage[s] = "Waiting" /\ Ready(s)
            /\ stage' = [stage EXCEPT ![s] = "Awake"]
            /\ UNCHANGED <<done, writer, restored>>
Acquire(s) == /\ stage[s] = "Awake" /\ writer = "Idle"
              /\ stage' = [stage EXCEPT ![s] = "Leased"]
              /\ UNCHANGED <<done, writer, restored>>
Recheck(s) == /\ stage[s] = "Leased"
              /\ stage' = [stage EXCEPT ![s] = IF SkipRecheck \/ Ready(s) THEN "Running" ELSE "Waiting"]
              /\ UNCHANGED <<done, writer, restored>>
Publish(s) == /\ stage[s] = "Running"
              /\ stage' = [stage EXCEPT ![s] = "Done"]
              /\ done' = done \cup {s}
              /\ UNCHANGED <<writer, restored>>
Cancel(s) == /\ stage[s] \in {"Waiting", "Awake", "Leased", "Running"}
             /\ stage' = [stage EXCEPT ![s] = "Idle"]
             /\ UNCHANGED <<done, writer, restored>>
RequestRestore == /\ writer = "Idle" /\ ~restored
                  /\ writer' = "Queued"
                  /\ UNCHANGED <<stage, done, restored>>
Restore == /\ writer = "Queued" /\ Leases = {}
           /\ done' = {}
           /\ stage' = [s \in Slots |-> IF stage[s] = "Done" THEN "Idle" ELSE stage[s]]
           /\ writer' = "Idle"
           /\ restored' = TRUE
Next == (\E s \in Slots : Request(s) \/ DirectReserve(s) \/ Awake(s) \/ Acquire(s) \/ Recheck(s) \/ Publish(s) \/ Cancel(s))
        \/ RequestRestore \/ Restore
Spec == Init /\ [][Next]_vars
TypeOK == /\ stage \in [Slots -> {"Idle", "Waiting", "Awake", "Leased", "Running", "Done"}]
          /\ done \subseteq Slots
          /\ writer \in {"Idle", "Queued"}
          /\ restored \in BOOLEAN
ExactCompletion == done = {s \in Slots : stage[s] = "Done"}
DependencySafety == \A s \in Slots : stage[s] \in {"Running", "Done"} => Ready(s)
QuiescentCaptureEnabled ==
  (writer = "Queued" /\ \A s \in Slots : stage[s] \notin {"Leased", "Running"}) => ENABLED Restore
IndependentProgress == \A s \in Slots :
  (stage[s] = "Waiting" /\ Predecessors(s) = {}) => ENABLED Awake(s)
=============================================================================
