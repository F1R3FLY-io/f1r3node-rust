------------------------- MODULE RetryOperationOwnership -------------------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS KeyCount, Incarnations, OperationCount, RequestCap, Budget, MaxRound, Unsafe
Keys == 1..KeyCount
Owners == Keys \X (1..Incarnations)
Operations == 1..OperationCount
EmptyOperation == [stage |-> "idle", owner |-> <<0, 0>>, peer |-> FALSE]

VARIABLES location, latest, received, total, peerTotal, expectedTotal, expectedPeer,
          deadline, reservationAllowed, operations, round, restarted
vars == <<location, latest, received, total, peerTotal, expectedTotal, expectedPeer,
          deadline, reservationAllowed, operations, round, restarted>>

Live(o) == location[o] \in {"active", "pending"}
Outstanding(o) == {p \in Operations : operations[p].owner = o
                    /\ operations[p].stage \in {"prepared", "sending"}}
Active == {o \in Owners : location[o] = "active"}

Init ==
    /\ location = [o \in Owners |-> "absent"]
    /\ latest = [k \in Keys |-> 0]
    /\ received = [o \in Owners |-> FALSE]
    /\ total = [o \in Owners |-> 0]
    /\ peerTotal = [o \in Owners |-> 0]
    /\ expectedTotal = [o \in Owners |-> 0]
    /\ expectedPeer = [o \in Owners |-> 0]
    /\ deadline = [o \in Owners |-> 0]
    /\ reservationAllowed = TRUE
    /\ operations = [p \in Operations |-> EmptyOperation]
    /\ round = 0 /\ restarted = FALSE

Admit(k) ==
    /\ latest[k] < Incarnations
    /\ ~\E o \in Owners : o[1] = k /\ Live(o)
    /\ Cardinality(Active) < RequestCap
    /\ latest' = [latest EXCEPT ![k] = @ + 1]
    /\ location' = [location EXCEPT ![<<k, latest[k] + 1>>] = "active"]
    /\ UNCHANGED <<received, total, peerTotal, expectedTotal, expectedPeer, deadline,
                    reservationAllowed, operations, round, restarted>>

Prepare(p, o, peer) ==
    /\ operations[p].stage = "idle" /\ Live(o) /\ ~received[o]
    /\ (deadline[o] = 0 /\ total[o] + (IF Unsafe = "unreserved-budget" THEN 0 ELSE Cardinality(Outstanding(o))) < Budget)
        \/ (Unsafe = "exhausted-reservation" /\ total[o] >= Budget /\ deadline[o] > 0 /\ round >= deadline[o])
    /\ operations' = [operations EXCEPT ![p] =
        [stage |-> "prepared", owner |-> o, peer |-> peer]]
    /\ total' = IF Unsafe = "precharge" THEN [total EXCEPT ![o] = @ + 1] ELSE total
    /\ reservationAllowed' = (reservationAllowed /\ total[o] < Budget)
    /\ UNCHANGED <<location, latest, received, peerTotal, expectedTotal, expectedPeer,
                    deadline, round, restarted>>

Quarantine(o) ==
    /\ Live(o) /\ total[o] >= Budget /\ deadline[o] = 0
    /\ deadline' = [deadline EXCEPT ![o] = round + 1]
    /\ UNCHANGED <<location, latest, received, total, peerTotal, expectedTotal,
                    expectedPeer, reservationAllowed, operations, round, restarted>>

Send(p) ==
    /\ operations[p].stage = "prepared"
    /\ operations' = [operations EXCEPT ![p].stage = "sending"]
    /\ UNCHANGED <<location, latest, received, total, peerTotal, expectedTotal,
                    expectedPeer, deadline, reservationAllowed, round, restarted>>

CompleteAction(p, returnedOk) ==
    /\ operations[p].stage = "sending"
        \/ (Unsafe = "double-completion" /\ operations[p].stage = "done")
    /\ LET origin == operations[p].owner
           target == IF Unsafe = "hash-only" THEN <<origin[1], latest[origin[1]]>> ELSE origin
           charge == Live(target) /\ (Unsafe # "forget-pending" \/ location[target] = "active")
                     /\ (Unsafe # "success-only" \/ returnedOk)
           expected == Live(origin) /\ operations[p].stage = "sending"
       IN
       /\ total' = IF charge THEN [total EXCEPT ![target] = @ + 1] ELSE total
       /\ peerTotal' = IF charge /\ operations[p].peer
                        THEN [peerTotal EXCEPT ![target] = @ + 1] ELSE peerTotal
       /\ expectedTotal' = IF expected THEN [expectedTotal EXCEPT ![origin] = @ + 1] ELSE expectedTotal
       /\ expectedPeer' = IF expected /\ operations[p].peer
                           THEN [expectedPeer EXCEPT ![origin] = @ + 1] ELSE expectedPeer
    /\ operations' = [operations EXCEPT ![p].stage = "done"]
    /\ UNCHANGED <<location, latest, received, deadline, reservationAllowed, round, restarted>>

Cancel(p) ==
    /\ operations[p].stage \in {"prepared", "sending"}
    /\ operations' = [operations EXCEPT ![p].stage = "done"]
    /\ UNCHANGED <<location, latest, received, total, peerTotal, expectedTotal,
                    expectedPeer, deadline, reservationAllowed, round, restarted>>

Release(p) ==
    /\ operations[p].stage = "done"
    /\ operations' = [operations EXCEPT ![p] = EmptyOperation]
    /\ UNCHANGED <<location, latest, received, total, peerTotal, expectedTotal,
                    expectedPeer, deadline, reservationAllowed, round, restarted>>

Pending(o) ==
    /\ location[o] = "active"
    /\ location' = [location EXCEPT ![o] = "pending"]
    /\ UNCHANGED <<latest, received, total, peerTotal, expectedTotal, expectedPeer,
                    deadline, reservationAllowed, operations, round, restarted>>

Activate(o) ==
    /\ location[o] = "pending" /\ Cardinality(Active) < RequestCap
    /\ location' = [location EXCEPT ![o] = "active"]
    /\ UNCHANGED <<latest, received, total, peerTotal, expectedTotal, expectedPeer,
                    deadline, reservationAllowed, operations, round, restarted>>

Finish(o) ==
    /\ Live(o)
    /\ location' = [location EXCEPT ![o] = "terminal"]
    /\ UNCHANGED <<latest, received, total, peerTotal, expectedTotal, expectedPeer,
                    deadline, reservationAllowed, operations, round, restarted>>

Receipt(o, value) ==
    /\ Live(o) /\ received[o] # value
    /\ received' = [received EXCEPT ![o] = value]
    /\ UNCHANGED <<location, latest, total, peerTotal, expectedTotal, expectedPeer,
                    deadline, reservationAllowed, operations, round, restarted>>

Tick ==
    /\ round < MaxRound /\ round' = round + 1
    /\ UNCHANGED <<location, latest, received, total, peerTotal, expectedTotal,
                    expectedPeer, deadline, reservationAllowed, operations, restarted>>

Restart ==
    /\ ~restarted /\ restarted' = TRUE
    /\ location' = [o \in Owners |-> IF location[o] = "active" THEN "terminal" ELSE location[o]]
    /\ operations' = [p \in Operations |-> EmptyOperation]
    /\ UNCHANGED <<latest, received, total, peerTotal, expectedTotal, expectedPeer,
                    deadline, reservationAllowed, round>>

Next == (\E k \in Keys : Admit(k))
        \/ (\E p \in Operations, o \in Owners, peer \in BOOLEAN : Prepare(p, o, peer))
        \/ (\E p \in Operations : Send(p) \/ CompleteAction(p, TRUE)
                                  \/ CompleteAction(p, FALSE) \/ Cancel(p) \/ Release(p))
        \/ (\E o \in Owners : Quarantine(o) \/ Pending(o) \/ Activate(o) \/ Finish(o)
                             \/ Receipt(o, TRUE) \/ Receipt(o, FALSE))
        \/ Tick \/ Restart

TypeOK ==
    /\ location \in [Owners -> {"absent", "active", "pending", "terminal"}]
    /\ latest \in [Keys -> 0..Incarnations] /\ received \in [Owners -> BOOLEAN]
    /\ total \in [Owners -> Nat] /\ peerTotal \in [Owners -> Nat]
    /\ expectedTotal \in [Owners -> Nat] /\ expectedPeer \in [Owners -> Nat]
    /\ deadline \in [Owners -> 0..(MaxRound + 1)]
    /\ reservationAllowed \in BOOLEAN
    /\ operations \in [Operations -> [stage : {"idle", "prepared", "sending", "done"},
         owner : Owners \cup {<<0, 0>>}, peer : BOOLEAN]]
    /\ round \in 0..MaxRound /\ restarted \in BOOLEAN

Inv_ExactCounters == total = expectedTotal /\ peerTotal = expectedPeer
Inv_Capacity == Cardinality(Active) <= RequestCap
Inv_OneOwner == \A k \in Keys : Cardinality({o \in Owners : o[1] = k /\ Live(o)}) <= 1
Inv_ReservedBudget == \A o \in Owners : Cardinality(Outstanding(o)) + total[o] <= Budget
Inv_NoExhaustedReservation == reservationAllowed
Inv_PeerAccounting == \A o \in Owners : peerTotal[o] <= total[o]
Spec == Init /\ [][Next]_vars
=============================================================================
