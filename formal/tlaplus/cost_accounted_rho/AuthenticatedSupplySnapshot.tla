---------------------- MODULE AuthenticatedSupplySnapshot ----------------------
EXTENDS Integers, TLC

CONSTANTS PreStateSupply, CandidateMint, Demand, Fee, ReadCandidateState

ASSUME /\ PreStateSupply \in Nat
       /\ CandidateMint \in Nat
       /\ Demand \in Nat
       /\ Fee \in Nat
       /\ ReadCandidateState \in BOOLEAN

VARIABLES phase, candidateStateSupply, observedSupply, admitted,
          checkpointed, committedSupply

vars == <<phase, candidateStateSupply, observedSupply, admitted,
          checkpointed, committedSupply>>

Init ==
  /\ phase = "Execute"
  /\ candidateStateSupply = PreStateSupply
  /\ observedSupply = 0
  /\ admitted = FALSE
  /\ checkpointed = FALSE
  /\ committedSupply = PreStateSupply

Execute ==
  /\ phase = "Execute"
  /\ candidateStateSupply' = PreStateSupply + CandidateMint
  /\ phase' = "Authenticate"
  /\ UNCHANGED <<observedSupply, admitted, checkpointed, committedSupply>>

Authenticate ==
  /\ phase = "Authenticate"
  /\ observedSupply' = IF ReadCandidateState
                         THEN candidateStateSupply
                         ELSE PreStateSupply
  /\ IF Demand + Fee <= IF ReadCandidateState
                           THEN candidateStateSupply
                           ELSE PreStateSupply
        THEN /\ admitted' = TRUE
             /\ checkpointed' = TRUE
             /\ committedSupply' = candidateStateSupply - (Demand + Fee)
        ELSE /\ admitted' = FALSE
             /\ checkpointed' = FALSE
             /\ candidateStateSupply' = PreStateSupply
             /\ committedSupply' = PreStateSupply
  /\ phase' = "Done"
  /\ (Demand + Fee <= IF ReadCandidateState
                        THEN candidateStateSupply
                        ELSE PreStateSupply) => UNCHANGED candidateStateSupply

Next == Execute \/ Authenticate

Spec == /\ Init
        /\ [][Next]_vars
        /\ WF_vars(Execute)
        /\ WF_vars(Authenticate)

TypeOK ==
  /\ phase \in {"Execute", "Authenticate", "Done"}
  /\ candidateStateSupply \in Nat
  /\ observedSupply \in Nat
  /\ admitted \in BOOLEAN
  /\ checkpointed \in BOOLEAN
  /\ committedSupply \in Nat

CandidateMintCannotFundItself ==
  admitted => Demand + Fee <= PreStateSupply

RejectedCandidateIsFullyRolledBack ==
  phase = "Done" /\ ~admitted =>
    /\ candidateStateSupply = PreStateSupply
    /\ committedSupply = PreStateSupply
    /\ ~checkpointed

AdmittedCandidateUsesAuthenticatedSupply ==
  phase = "Done" /\ admitted => observedSupply = PreStateSupply

EventuallyDone == <>(phase = "Done")

=============================================================================
