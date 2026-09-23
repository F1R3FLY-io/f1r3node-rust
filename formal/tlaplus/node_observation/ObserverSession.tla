-------------------------- MODULE ObserverSession --------------------------
EXTENDS Naturals, Sequences, FiniteSets

CONSTANTS MaxSessions, Nonces, BreakFreshness
VARIABLES phase, sessions, sequence, challenges, acceptedOrigin

vars == <<phase, sessions, sequence, challenges, acceptedOrigin>>

Init == /\ phase = "idle"
        /\ sessions = 0
        /\ sequence = 0
        /\ challenges = <<>>
        /\ acceptedOrigin = 0

Hello(nonce) ==
    /\ phase = "idle"
    /\ sessions < MaxSessions
    /\ phase' = "request"
    /\ sessions' = sessions + 1
    /\ sequence' = sequence + 1
    /\ challenges' = Append(challenges,
          <<nonce, IF BreakFreshness THEN 0 ELSE sequence + 1>>)
    /\ acceptedOrigin' = 0

Request(origin) ==
    /\ phase = "request"
    /\ origin \in 1..Len(challenges)
    /\ LET accepted == challenges[origin] = challenges[Len(challenges)]
       IN /\ acceptedOrigin' = IF accepted THEN origin ELSE 0
          /\ sequence' = IF accepted THEN sequence + 1 ELSE sequence
    /\ phase' = "idle"
    /\ UNCHANGED <<sessions, challenges>>

Disconnect ==
    /\ phase = "request"
    /\ phase' = "idle"
    /\ acceptedOrigin' = 0
    /\ UNCHANGED <<sessions, sequence, challenges>>

Next == (\E nonce \in Nonces : Hello(nonce))
        \/ (\E origin \in 1..Len(challenges) : Request(origin))
        \/ Disconnect

TypeOK == /\ phase \in {"idle", "request"}
          /\ sessions \in 0..MaxSessions
          /\ sequence \in 0..(2 * MaxSessions)
          /\ Len(challenges) = sessions
          /\ challenges \in Seq(Nonces \X (0..(2 * MaxSessions)))
          /\ acceptedOrigin \in 0..sessions

FreshChallenges == \A i, j \in 1..Len(challenges) :
                       challenges[i] = challenges[j] => i = j

ReplayRefused == acceptedOrigin = 0 \/ acceptedOrigin = Len(challenges)

Spec == Init /\ [][Next]_vars
=============================================================================
