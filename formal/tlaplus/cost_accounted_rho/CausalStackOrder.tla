---------------------------- MODULE CausalStackOrder ----------------------------
EXTENDS Naturals, Sequences, TLC

CONSTANT HashSortsInsteadOfCausalOrder

ASSUME HashSortsInsteadOfCausalOrder \in BOOLEAN

CausalTrace == <<"a", "b">>
IdentitySortedTrace == <<"b", "a">>
InitialStack == <<"a", "b">>
CertifiedTrace ==
    IF HashSortsInsteadOfCausalOrder THEN IdentitySortedTrace ELSE CausalTrace

VARIABLES phase, settleIndex, stackPosition, replayIndex, replayPosition,
          accepted, rejected

vars == <<phase, settleIndex, stackPosition, replayIndex, replayPosition,
          accepted, rejected>>

Init ==
    /\ phase = "Settle"
    /\ settleIndex = 1
    /\ stackPosition = 1
    /\ replayIndex = 1
    /\ replayPosition = 1
    /\ accepted = FALSE
    /\ rejected = FALSE

Settle ==
    /\ phase = "Settle"
    /\ settleIndex <= Len(CertifiedTrace)
    /\ IF InitialStack[stackPosition] = CertifiedTrace[settleIndex]
       THEN /\ stackPosition' = stackPosition + 1
            /\ settleIndex' = settleIndex + 1
            /\ phase' = IF settleIndex' > Len(CertifiedTrace) THEN "Replay" ELSE phase
            /\ accepted' = (settleIndex' > Len(CertifiedTrace))
            /\ UNCHANGED rejected
       ELSE /\ rejected' = TRUE
            /\ phase' = "Done"
            /\ UNCHANGED <<settleIndex, stackPosition, accepted>>
    /\ UNCHANGED <<replayIndex, replayPosition>>

Replay ==
    /\ phase = "Replay"
    /\ replayIndex <= Len(CertifiedTrace)
    /\ InitialStack[replayPosition] = CertifiedTrace[replayIndex]
    /\ replayPosition' = replayPosition + 1
    /\ replayIndex' = replayIndex + 1
    /\ phase' = IF replayIndex' > Len(CertifiedTrace) THEN "Done" ELSE phase
    /\ UNCHANGED <<settleIndex, stackPosition, accepted, rejected>>

Next == Settle \/ Replay
Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ phase \in {"Settle", "Replay", "Done"}
    /\ settleIndex \in 1..(Len(CertifiedTrace) + 1)
    /\ stackPosition \in 1..(Len(InitialStack) + 1)
    /\ replayIndex \in 1..(Len(CertifiedTrace) + 1)
    /\ replayPosition \in 1..(Len(InitialStack) + 1)
    /\ accepted \in BOOLEAN
    /\ rejected \in BOOLEAN

CausallyFundedTraceIsAccepted == ~rejected

EverySettledEventPopsOneHead ==
    phase \in {"Settle", "Replay"} => stackPosition = settleIndex

CertificatePreservesCausalOrder == accepted => CertifiedTrace = CausalTrace

ReplayUsesCertifiedOrder ==
    phase = "Done" /\ accepted => replayPosition = stackPosition

=============================================================================
