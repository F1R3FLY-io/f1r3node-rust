---------------------- MODULE CertifiedCarrierPublication ----------------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS Validators, Carriers, FaultyValidator,
          SelectAmbientUnsafe, WakeUnsafe, LocalStopUnsafe

VARIABLES ready, causal, ambient, parked, requested, appended,
          chosen, closureAtAppend, dagGeneration

vars == << ready, causal, ambient, parked, requested, appended,
           chosen, closureAtAppend, dagGeneration >>

None == 0

Least(S) == CHOOSE c \in S : \A d \in S : c <= d

EffectiveCausal(v) ==
    IF LocalStopUnsafe /\ v = FaultyValidator /\ causal[v] # {}
    THEN causal[v] \ {Least(causal[v])}
    ELSE causal[v]

Available(v) ==
    IF SelectAmbientUnsafe
    THEN EffectiveCausal(v) \cup ambient[v]
    ELSE EffectiveCausal(v)

Init ==
    /\ ready = [v \in Validators |-> FALSE]
    /\ causal = [v \in Validators |-> {}]
    /\ ambient = [v \in Validators |-> {}]
    /\ parked = [v \in Validators |-> FALSE]
    /\ requested = [v \in Validators |-> FALSE]
    /\ appended = [v \in Validators |-> FALSE]
    /\ chosen = [v \in Validators |-> None]
    /\ closureAtAppend = [v \in Validators |-> {}]
    /\ dagGeneration = [v \in Validators |-> 0]

Ready(v) ==
    /\ ~ready[v]
    /\ ready' = [ready EXCEPT ![v] = TRUE]
    /\ UNCHANGED << causal, ambient, parked, requested, appended,
                    chosen, closureAtAppend, dagGeneration >>

AdmitCausal(v, c) ==
    /\ c \notin causal[v]
    /\ causal' = [causal EXCEPT ![v] = @ \cup {c}]
    /\ dagGeneration' = [dagGeneration EXCEPT ![v] = @ + 1]
    /\ IF parked[v] /\ ~WakeUnsafe
          THEN /\ parked' = [parked EXCEPT ![v] = FALSE]
               /\ requested' = [requested EXCEPT ![v] = TRUE]
          ELSE /\ UNCHANGED parked
               /\ UNCHANGED requested
    /\ UNCHANGED << ready, ambient, appended, chosen, closureAtAppend >>

AdmitAmbient(v, c) ==
    /\ c \notin ambient[v]
    /\ ambient' = [ambient EXCEPT ![v] = @ \cup {c}]
    /\ dagGeneration' = [dagGeneration EXCEPT ![v] = @ + 1]
    /\ UNCHANGED << ready, causal, parked, requested, appended,
                    chosen, closureAtAppend >>

Evaluate(v) ==
    /\ ready[v]
    /\ ~appended[v]
    /\ IF Available(v) = {}
          THEN /\ parked' = [parked EXCEPT ![v] = TRUE]
               /\ requested' = [requested EXCEPT ![v] = FALSE]
               /\ UNCHANGED << appended, chosen, closureAtAppend >>
          ELSE /\ appended' = [appended EXCEPT ![v] = TRUE]
               /\ chosen' = [chosen EXCEPT ![v] = Least(Available(v))]
               /\ closureAtAppend' = [closureAtAppend EXCEPT ![v] = causal[v]]
               /\ parked' = [parked EXCEPT ![v] = FALSE]
               /\ requested' = [requested EXCEPT ![v] = FALSE]
    /\ UNCHANGED << ready, causal, ambient, dagGeneration >>

Restart(v) ==
    /\ ~appended[v]
    /\ parked' = [parked EXCEPT ![v] = FALSE]
    /\ requested' = [requested EXCEPT ![v] = FALSE]
    /\ UNCHANGED << ready, causal, ambient, appended,
                    chosen, closureAtAppend, dagGeneration >>

Next ==
    \/ \E v \in Validators : Ready(v)
    \/ \E v \in Validators, c \in Carriers : AdmitCausal(v, c)
    \/ \E v \in Validators, c \in Carriers : AdmitAmbient(v, c)
    \/ \E v \in Validators : Evaluate(v)
    \/ \E v \in Validators : Restart(v)

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ ready \in [Validators -> BOOLEAN]
    /\ causal \in [Validators -> SUBSET Carriers]
    /\ ambient \in [Validators -> SUBSET Carriers]
    /\ parked \in [Validators -> BOOLEAN]
    /\ requested \in [Validators -> BOOLEAN]
    /\ appended \in [Validators -> BOOLEAN]
    /\ chosen \in [Validators -> Carriers \cup {None}]
    /\ closureAtAppend \in [Validators -> SUBSET Carriers]
    /\ dagGeneration \in [Validators -> Nat]

ChosenCarrierIsCertified ==
    \A v \in Validators : appended[v] => chosen[v] \in closureAtAppend[v]

ChosenCarrierIsCanonicalMinimum ==
    \A v \in Validators :
        appended[v] => \A c \in closureAtAppend[v] : chosen[v] <= c

EquivalentClosuresChooseSameCarrier ==
    \A left, right \in Validators :
        appended[left] /\ appended[right]
        /\ closureAtAppend[left] = closureAtAppend[right]
        => chosen[left] = chosen[right]

CarrierAdmissionReleasesParking ==
    \A v \in Validators :
        parked[v] /\ causal[v] # {} => requested[v] \/ appended[v]

AmbientCarrierCannotAdvance ==
    \A v \in Validators :
        appended[v] => chosen[v] \notin (ambient[v] \ closureAtAppend[v])

=============================================================================
