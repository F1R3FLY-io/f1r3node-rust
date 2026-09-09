--------------------------- MODULE BufferRetryHandoff ---------------------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS BlockCount, WorkerCount, RequestCap, RetryLimit, Unsafe

Blocks == 1..BlockCount
Workers == 1..WorkerCount
Phases == {"idle", "reserved", "prepared", "writing", "failed", "reopened", "committed", "acked"}
Seeded == IF RequestCap = 0 THEN {} ELSE {1}

VARIABLES tracked, ready, durable, terminal, seen, acknowledged, owner, phase,
          policy, expectedPolicy, pendingReceipt

vars == <<tracked, ready, durable, terminal, seen, acknowledged, owner, phase,
          policy, expectedPolicy, pendingReceipt>>

PolicyValues == [dependency : BOOLEAN, attempts : 0..RetryLimit,
                 quarantine : BOOLEAN, cooldown : BOOLEAN, initialAge : 0..1]
FreshPolicy == [dependency |-> FALSE, attempts |-> 0,
                quarantine |-> FALSE, cooldown |-> FALSE, initialAge |-> 0]
Owners == {owner[w] : w \in {v \in Workers : phase[v] # "idle"}}
OtherOwners(w) == {owner[v] : v \in {x \in Workers \ {w} : phase[x] # "idle"}}
CanTrack(b) == b \in tracked \/ Cardinality(tracked) < RequestCap
HasSuccessor(b) == b \in durable \cup terminal \cup ready

Init ==
    /\ tracked = Seeded
    /\ ready = Seeded
    /\ durable = {}
    /\ terminal = {}
    /\ seen = Seeded
    /\ acknowledged = {}
    /\ owner = [w \in Workers |-> 0]
    /\ phase = [w \in Workers |-> "idle"]
    /\ policy = [b \in Blocks |->
        IF b \in Seeded THEN
          [dependency |-> TRUE, attempts |-> RetryLimit,
           quarantine |-> TRUE, cooldown |-> TRUE, initialAge |-> 1]
        ELSE FreshPolicy]
    /\ expectedPolicy = policy
    /\ pendingReceipt = {}

Reserve(w, b) ==
    /\ phase[w] = "idle"
    /\ b \notin Owners
    /\ owner' = [owner EXCEPT ![w] = b]
    /\ phase' = [phase EXCEPT ![w] = "reserved"]
    /\ UNCHANGED <<tracked, ready, durable, terminal, seen, acknowledged,
                   policy, expectedPolicy, pendingReceipt>>

CancelReservation(w) ==
    /\ phase[w] = "reserved"
    /\ owner' = [owner EXCEPT ![w] = 0]
    /\ phase' = [phase EXCEPT ![w] = "idle"]
    /\ UNCHANGED <<tracked, ready, durable, terminal, seen, acknowledged,
                   policy, expectedPolicy, pendingReceipt>>

Receive(w) ==
    /\ phase[w] = "reserved"
    /\ tracked' = IF CanTrack(owner[w]) THEN tracked \cup {owner[w]} ELSE tracked
    /\ ready' = ready \ {owner[w]}
    /\ phase' = [phase EXCEPT ![w] = "prepared"]
    /\ policy' = IF Unsafe = "receipt-resets-budget"
        THEN [policy EXCEPT ![owner[w]].attempts = 0]
        ELSE policy
    /\ UNCHANGED <<durable, terminal, seen, acknowledged, owner,
                   expectedPolicy, pendingReceipt>>

Publish(w) ==
    /\ phase[w] = "prepared" \/ (Unsafe = "late-receipt" /\ phase[w] = "reserved")
    /\ pendingReceipt' = IF phase[w] = "reserved"
        THEN pendingReceipt \cup {owner[w]} ELSE pendingReceipt
    /\ phase' = [phase EXCEPT ![w] = "writing"]
    /\ seen' = seen \cup {owner[w]}
    /\ UNCHANGED <<tracked, ready, durable, terminal, acknowledged,
                   owner, policy, expectedPolicy>>

LateReceipt(b) ==
    /\ b \in pendingReceipt
    /\ tracked' = IF CanTrack(b) THEN tracked \cup {b} ELSE tracked
    /\ ready' = ready \ {b}
    /\ pendingReceipt' = pendingReceipt \ {b}
    /\ UNCHANGED <<durable, terminal, seen, acknowledged, owner, phase,
                   policy, expectedPolicy>>

FailWrite(w) ==
    /\ phase[w] = "writing"
    /\ phase' = [phase EXCEPT ![w] = "failed"]
    /\ UNCHANGED <<tracked, ready, durable, terminal, seen, acknowledged,
                   owner, policy, expectedPolicy, pendingReceipt>>

Commit(w) ==
    /\ phase[w] = "writing"
    /\ durable' = durable \cup {owner[w]}
    /\ phase' = [phase EXCEPT ![w] = "committed"]
    /\ UNCHANGED <<tracked, ready, terminal, seen, acknowledged,
                   owner, policy, expectedPolicy, pendingReceipt>>

Reopen(w) ==
    /\ phase[w] = "failed"
    /\ CanTrack(owner[w])
    /\ tracked' = tracked \cup {owner[w]}
    /\ ready' = IF Unsafe = "received-is-retry" THEN ready ELSE ready \cup {owner[w]}
    /\ phase' = [phase EXCEPT ![w] = "reopened"]
    /\ policy' = CASE Unsafe = "invent-dependency" ->
                        [policy EXCEPT ![owner[w]].dependency = TRUE]
                   [] Unsafe = "reset-budget" ->
                        [policy EXCEPT ![owner[w]].attempts = 0]
                   [] Unsafe = "clear-quarantine" ->
                        [policy EXCEPT ![owner[w]].quarantine = FALSE]
                   [] Unsafe = "clear-cooldown" ->
                        [policy EXCEPT ![owner[w]].cooldown = FALSE]
                   [] Unsafe = "reset-age" ->
                        [policy EXCEPT ![owner[w]].initialAge = 0]
                   [] OTHER -> policy
    /\ UNCHANGED <<durable, terminal, seen, acknowledged, owner,
                   expectedPolicy, pendingReceipt>>

RetryPublication(w) ==
    /\ phase[w] = "failed"
    /\ Unsafe # "wait-only-for-capacity" \/ CanTrack(owner[w])
    /\ phase' = [phase EXCEPT ![w] = "writing"]
    /\ UNCHANGED <<tracked, ready, durable, terminal, seen, acknowledged,
                   owner, policy, expectedPolicy, pendingReceipt>>

Acknowledge(w) ==
    /\ phase[w] \in {"committed", "failed"}
    /\ owner[w] \in durable \cup terminal
    /\ tracked' = tracked \ {owner[w]}
    /\ ready' = ready \ {owner[w]}
    /\ acknowledged' = acknowledged \cup {owner[w]}
    /\ phase' = [phase EXCEPT ![w] = "acked"]
    /\ UNCHANGED <<durable, terminal, seen, owner, policy, expectedPolicy, pendingReceipt>>

Release(w) ==
    /\ phase[w] \in {"failed", "reopened", "acked"}
    /\ HasSuccessor(owner[w])
        \/ (Unsafe = "release-at-capacity" /\ ~CanTrack(owner[w]))
        \/ (Unsafe = "received-is-retry" /\ owner[w] \in tracked)
    /\ owner' = [owner EXCEPT ![w] = 0]
    /\ phase' = [phase EXCEPT ![w] = "idle"]
    /\ UNCHANGED <<tracked, ready, durable, terminal, seen, acknowledged,
                   policy, expectedPolicy, pendingReceipt>>

Resolve(b) ==
    /\ b \in seen
    /\ b \notin terminal
    /\ terminal' = terminal \cup {b}
    /\ durable' = durable \ {b}
    /\ tracked' = tracked \ {b}
    /\ ready' = ready \ {b}
    /\ UNCHANGED <<seen, acknowledged, owner, phase, policy, expectedPolicy, pendingReceipt>>

RetryRequest(b) ==
    /\ b \in ready
    /\ ~policy[b].quarantine
    /\ ~policy[b].cooldown
    /\ policy[b].attempts < RetryLimit
    /\ policy' = [policy EXCEPT ![b].attempts = @ + 1, ![b].cooldown = TRUE]
    /\ expectedPolicy' = [expectedPolicy EXCEPT ![b].attempts = @ + 1, ![b].cooldown = TRUE]
    /\ UNCHANGED <<tracked, ready, durable, terminal, seen, acknowledged,
                   owner, phase, pendingReceipt>>

ExpireCooldown(b) ==
    /\ policy[b].cooldown
    /\ policy' = [policy EXCEPT ![b].cooldown = FALSE]
    /\ expectedPolicy' = [expectedPolicy EXCEPT ![b].cooldown = FALSE]
    /\ UNCHANGED <<tracked, ready, durable, terminal, seen, acknowledged,
                   owner, phase, pendingReceipt>>

Restart ==
    /\ tracked' = {}
    /\ ready' = {}
    /\ seen' = durable \cup terminal \cup acknowledged
    /\ owner' = [w \in Workers |-> 0]
    /\ phase' = [w \in Workers |-> "idle"]
    /\ policy' = [b \in Blocks |-> FreshPolicy]
    /\ expectedPolicy' = policy'
    /\ pendingReceipt' = {}
    /\ UNCHANGED <<durable, terminal, acknowledged>>

Next ==
    \/ \E w \in Workers, b \in Blocks : Reserve(w, b)
    \/ \E w \in Workers : CancelReservation(w) \/ Receive(w) \/ Publish(w) \/ FailWrite(w) \/ Commit(w)
        \/ Reopen(w) \/ RetryPublication(w) \/ Acknowledge(w) \/ Release(w)
    \/ \E b \in Blocks : LateReceipt(b) \/ Resolve(b) \/ RetryRequest(b) \/ ExpireCooldown(b)
    \/ Restart

TypeOK ==
    /\ tracked \subseteq Blocks
    /\ ready \subseteq tracked
    /\ durable \subseteq Blocks
    /\ terminal \subseteq Blocks
    /\ seen \subseteq Blocks
    /\ acknowledged \subseteq Blocks
    /\ owner \in [Workers -> 0..BlockCount]
    /\ phase \in [Workers -> Phases]
    /\ policy \in [Blocks -> PolicyValues]
    /\ expectedPolicy \in [Blocks -> PolicyValues]
    /\ pendingReceipt \subseteq Blocks
    /\ \A w \in Workers : (owner[w] = 0) <=> (phase[w] = "idle")

Inv_LiveOwner == seen \subseteq durable \cup terminal \cup ready \cup Owners
Inv_TrackerBound == Cardinality(tracked) <= RequestCap
Inv_UniqueLease == \A w \in Workers : owner[w] = 0 \/ owner[w] \notin OtherOwners(w)
Inv_RetryPolicy == policy = expectedPolicy
Inv_AcknowledgedDurability == acknowledged \subseteq durable \cup terminal
Inv_LocalRetryAvailable == \A w \in Workers :
    phase[w] = "failed" /\ ~HasSuccessor(owner[w]) /\ ~CanTrack(owner[w])
        => ENABLED RetryPublication(w)

Spec == Init /\ [][Next]_vars
=============================================================================
