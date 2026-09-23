------------------------- MODULE ObserverSession -------------------------
EXTENDS Naturals, Sequences, FiniteSets
CONSTANTS MaxSessions, Deadline, MaxFrame, Bug
VARIABLES phase, sessions, elapsed, replies
vars == <<phase, sessions, elapsed, replies>>
Init == /\ phase = "listen" /\ sessions = 0 /\ elapsed = 0 /\ replies = <<>>
Begin ==
    /\ phase = "listen"
    /\ sessions < MaxSessions + (IF Bug = "budget" THEN 1 ELSE 0)
    /\ phase' = "session" /\ sessions' = sessions + 1 /\ elapsed' = 0
    /\ UNCHANGED replies
Tick == /\ phase = "session" /\ elapsed < Deadline
        /\ elapsed' = elapsed + 1 /\ UNCHANGED <<phase, sessions, replies>>
Close == /\ phase = "session" /\ phase' = "listen"
         /\ UNCHANGED <<sessions, elapsed, replies>>
Reply(nonce, peer, identity, size) ==
    /\ phase = "session" /\ Len(replies) < MaxSessions + 1
    /\ (nonce = sessions \/ Bug = "challenge")
    /\ ((peer /\ identity) \/ Bug = "identity")
    /\ ((size > 0 /\ size <= MaxFrame) \/ Bug = "frame")
    /\ (elapsed < Deadline \/ Bug = "deadline")
    /\ replies' = Append(replies, [session |-> sessions, challenge |-> nonce,
                                 peer |-> peer, identity |-> identity,
                                 bytes |-> size, at |-> elapsed])
    /\ phase' = (IF Bug = "repeat" THEN "session" ELSE "listen")
    /\ UNCHANGED <<sessions, elapsed>>
Next == Begin \/ Tick \/ Close \/
        \E nonce \in 0..(MaxSessions + 1), peer \in BOOLEAN,
           identity \in BOOLEAN, size \in {0, MaxFrame, MaxFrame + 1}:
              Reply(nonce, peer, identity, size)
Spec == Init /\ [][Next]_vars
TypeOK == /\ phase \in {"listen", "session"}
          /\ sessions \in 0..(MaxSessions + 1) /\ elapsed \in 0..Deadline
          /\ Len(replies) <= MaxSessions + 1
FreshChallenge == \A i \in 1..Len(replies): replies[i].challenge = replies[i].session
BoundIdentity == \A i \in 1..Len(replies): replies[i].peer /\ replies[i].identity
BoundFrame == \A i \in 1..Len(replies): replies[i].bytes > 0 /\ replies[i].bytes <= MaxFrame
BoundDeadline == \A i \in 1..Len(replies): replies[i].at < Deadline
OneRequest == \A i, j \in 1..Len(replies): replies[i].session = replies[j].session => i = j
SessionBudget == sessions <= MaxSessions
=============================================================================
