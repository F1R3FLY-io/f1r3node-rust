------------------------- MODULE ObserverSession -------------------------
EXTENDS Naturals, Sequences, FiniteSets

CONSTANTS MaxSessions, Deadline, MaxFrame, Nonces, BreakFreshness, Bug
VARIABLES phase, sessions, elapsed, sequence, challenges, replies
vars == <<phase, sessions, elapsed, sequence, challenges, replies>>

Init == /\ phase = "listen"
        /\ sessions = 0
        /\ elapsed = 0
        /\ sequence = 0
        /\ challenges = <<>>
        /\ replies = <<>>

Begin(nonce) ==
    /\ phase = "listen"
    /\ sessions < MaxSessions + (IF Bug = "budget" THEN 1 ELSE 0)
    /\ nonce \in Nonces
    /\ phase' = "session"
    /\ sessions' = sessions + 1
    /\ elapsed' = 0
    /\ sequence' = sequence + 1
    /\ challenges' = Append(challenges,
          <<nonce, IF BreakFreshness THEN 0 ELSE sequence + 1>>)
    /\ UNCHANGED replies

Tick == /\ phase = "session" /\ elapsed < Deadline
        /\ elapsed' = elapsed + 1
        /\ UNCHANGED <<phase, sessions, sequence, challenges, replies>>

Close == /\ phase = "session" /\ phase' = "listen"
         /\ UNCHANGED <<sessions, elapsed, sequence, challenges, replies>>

Token(origin) == IF origin \in 1..Len(challenges)
                 THEN challenges[origin] ELSE <<"unissued", 0>>

Reply(origin, peer, identity, size) ==
    /\ phase = "session" /\ Len(replies) < MaxSessions + 1
    /\ (Token(origin) = challenges[sessions] \/ Bug = "challenge")
    /\ ((peer /\ identity) \/ Bug = "identity")
    /\ ((size > 0 /\ size <= MaxFrame) \/ Bug = "frame")
    /\ (elapsed < Deadline \/ Bug = "deadline")
    /\ replies' = Append(replies,
          [session |-> sessions, origin |-> origin, challenge |-> Token(origin),
           peer |-> peer, identity |-> identity, bytes |-> size, at |-> elapsed])
    /\ sequence' = sequence + 1
    /\ phase' = (IF Bug = "repeat" THEN "session" ELSE "listen")
    /\ UNCHANGED <<sessions, elapsed, challenges>>

Next == (\E nonce \in Nonces: Begin(nonce)) \/ Tick \/ Close \/
        (\E origin \in 0..(MaxSessions + 1), peer \in BOOLEAN,
            identity \in BOOLEAN, size \in {0, MaxFrame, MaxFrame + 1}:
                Reply(origin, peer, identity, size))

Spec == Init /\ [][Next]_vars
TypeOK == /\ phase \in {"listen", "session"}
          /\ sessions \in 0..(MaxSessions + 1)
          /\ elapsed \in 0..Deadline
          /\ sequence \in 0..(2 * (MaxSessions + 1))
          /\ Len(challenges) = sessions
          /\ challenges \in Seq(Nonces \X (0..(2 * (MaxSessions + 1))))
          /\ Len(replies) <= MaxSessions + 1
FreshChallenges == \A i, j \in 1..Len(challenges):
                       challenges[i] = challenges[j] => i = j
FreshChallenge == \A i \in 1..Len(replies):
                      replies[i].challenge = challenges[replies[i].session]
ReplayRefused == \A i \in 1..Len(replies): replies[i].origin = replies[i].session
BoundIdentity == \A i \in 1..Len(replies): replies[i].peer /\ replies[i].identity
BoundFrame == \A i \in 1..Len(replies): replies[i].bytes > 0 /\ replies[i].bytes <= MaxFrame
BoundDeadline == \A i \in 1..Len(replies): replies[i].at < Deadline
OneRequest == \A i, j \in 1..Len(replies): replies[i].session = replies[j].session => i = j
SessionBudget == sessions <= MaxSessions
=============================================================================
