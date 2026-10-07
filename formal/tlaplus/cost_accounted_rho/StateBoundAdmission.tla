---------------------------- MODULE StateBoundAdmission ----------------------------
EXTENDS Integers, Sequences, TLC

CONSTANTS Events, Schedules, EventCost, InitialSupply, Fee,
          StructuralReservation, UseStructuralProof,
          AllowExecutionDrift, AllowExhaustedAdmission,
          ReplaySubstitutesEvidence, PersistBeforePhysicalSettlement

ASSUME /\ Events # {}
       /\ Schedules \subseteq Seq(Events)
       /\ Schedules # {}
       /\ EventCost \in [Events -> Nat]
       /\ InitialSupply \in Nat
       /\ Fee \in Nat
       /\ StructuralReservation \in Nat
       /\ UseStructuralProof \in BOOLEAN
       /\ AllowExecutionDrift \in BOOLEAN
       /\ AllowExhaustedAdmission \in BOOLEAN
       /\ ReplaySubstitutesEvidence \in BOOLEAN
       /\ PersistBeforePhysicalSettlement \in BOOLEAN

VARIABLES phase, proofCost, commitCost, replayCost,
          proofPost, commitPost, replayPost, supply,
          preflightSchedule, commitSchedule, replaySchedule, completed,
          executionCount, candidateCheckpointed

vars == <<phase, proofCost, commitCost, replayCost,
          proofPost, commitPost, replayPost, supply,
          preflightSchedule, commitSchedule, replaySchedule, completed,
          executionCount, candidateCheckpointed>>

RECURSIVE ScheduleCost(_)
RECURSIVE EventSet(_)

ScheduleCost(schedule) ==
  IF Len(schedule) = 0
  THEN 0
  ELSE EventCost[Head(schedule)] + ScheduleCost(Tail(schedule))

EventSet(schedule) ==
  IF Len(schedule) = 0
  THEN {}
  ELSE {Head(schedule)} \cup EventSet(Tail(schedule))

Capacity == InitialSupply - Fee

Init ==
  /\ phase = "Preflight"
  /\ proofCost = 0
  /\ commitCost = 0
  /\ replayCost = 0
  /\ proofPost = <<>>
  /\ commitPost = <<>>
  /\ replayPost = <<>>
  /\ supply = InitialSupply
  /\ preflightSchedule \in Schedules
  /\ commitSchedule \in Schedules
  /\ replaySchedule \in Schedules
  /\ completed = FALSE
  /\ executionCount = 0
  /\ candidateCheckpointed = FALSE

Preflight ==
  /\ phase = "Preflight"
  /\ IF UseStructuralProof
        THEN /\ proofCost' = StructuralReservation
             /\ proofPost' = <<>>
             /\ completed' = TRUE
             /\ phase' = "Admission"
             /\ UNCHANGED candidateCheckpointed
        ELSE LET observed == ScheduleCost(preflightSchedule)
             IN IF observed <= Capacity
                   THEN /\ proofCost' = observed
                        /\ proofPost' = preflightSchedule
                        /\ completed' = TRUE
                        /\ executionCount' = executionCount + 1
                        /\ candidateCheckpointed' = PersistBeforePhysicalSettlement
                        /\ phase' = "Admission"
                   ELSE /\ proofCost' = observed
                        /\ proofPost' = preflightSchedule
                        /\ completed' = FALSE
                        /\ executionCount' = executionCount + 1
                        /\ candidateCheckpointed' = PersistBeforePhysicalSettlement
                        /\ phase' = IF AllowExhaustedAdmission
                                       THEN "Admission"
                                       ELSE "Rejected"
  /\ UNCHANGED <<commitCost, replayCost, commitPost, replayPost, supply, preflightSchedule,
                  commitSchedule, replaySchedule>>
  /\ UseStructuralProof => UNCHANGED executionCount

Admit ==
  /\ phase = "Admission"
  /\ proofCost + Fee <= InitialSupply
  /\ IF UseStructuralProof \/ AllowExecutionDrift
        THEN /\ phase' = "Commit"
             /\ UNCHANGED candidateCheckpointed
        ELSE /\ commitCost' = proofCost
             /\ commitPost' = proofPost
             /\ candidateCheckpointed' = TRUE
             /\ phase' = "Replay"
  /\ UNCHANGED <<proofCost, replayCost, proofPost, replayPost, supply,
                  preflightSchedule, commitSchedule, replaySchedule, completed,
                  executionCount>>
  /\ (UseStructuralProof \/ AllowExecutionDrift) => UNCHANGED <<commitCost, commitPost>>

RejectUnderfunded ==
  /\ phase = "Admission"
  /\ proofCost + Fee > InitialSupply
  /\ phase' = "Rejected"
  /\ UNCHANGED <<proofCost, commitCost, replayCost, proofPost, commitPost, replayPost, supply,
                  preflightSchedule, commitSchedule, replaySchedule, completed,
                  executionCount, candidateCheckpointed>>

Commit ==
  /\ phase = "Commit"
  /\ commitCost' = IF AllowExecutionDrift
                      THEN ScheduleCost(Tail(commitSchedule))
                      ELSE ScheduleCost(commitSchedule)
  /\ commitPost' = IF AllowExecutionDrift
                      THEN Tail(commitSchedule)
                      ELSE commitSchedule
  /\ executionCount' = executionCount + 1
  /\ candidateCheckpointed' = TRUE
  /\ phase' = "Replay"
  /\ UNCHANGED <<proofCost, replayCost, proofPost, replayPost, supply, preflightSchedule,
                  commitSchedule, replaySchedule, completed>>

Replay ==
  /\ phase = "Replay"
  /\ replayCost' = IF ReplaySubstitutesEvidence
                      THEN ScheduleCost(Tail(replaySchedule))
                      ELSE commitCost
  /\ replayPost' = IF ReplaySubstitutesEvidence
                      THEN Tail(replaySchedule)
                      ELSE commitPost
  /\ phase' = "Settlement"
  /\ UNCHANGED <<proofCost, commitCost, proofPost, commitPost, supply, preflightSchedule,
                  commitSchedule, replaySchedule, completed, executionCount,
                  candidateCheckpointed>>

Settle ==
  /\ phase = "Settlement"
  /\ IF UseStructuralProof THEN commitCost <= proofCost ELSE proofCost = commitCost
  /\ commitCost = replayCost
  /\ IF UseStructuralProof THEN TRUE ELSE proofPost = commitPost
  /\ commitPost = replayPost
  /\ supply' = InitialSupply - (commitCost + Fee)
  /\ phase' = "Done"
  /\ UNCHANGED <<proofCost, commitCost, replayCost, proofPost, commitPost, replayPost, preflightSchedule,
                  commitSchedule, replaySchedule, completed, executionCount,
                  candidateCheckpointed>>

RejectDrift ==
  /\ phase = "Settlement"
  /\ \/ IF UseStructuralProof THEN commitCost > proofCost ELSE proofCost # commitCost
     \/ commitCost # replayCost
     \/ IF UseStructuralProof THEN FALSE ELSE proofPost # commitPost
     \/ commitPost # replayPost
  /\ phase' = "Rejected"
  /\ UNCHANGED <<proofCost, commitCost, replayCost, proofPost, commitPost, replayPost, supply,
                  preflightSchedule, commitSchedule, replaySchedule, completed,
                  executionCount, candidateCheckpointed>>

Next == Preflight \/ Admit \/ RejectUnderfunded \/ Commit \/ Replay \/ Settle \/ RejectDrift

Spec == /\ Init
        /\ [][Next]_vars
        /\ WF_vars(Preflight)
        /\ WF_vars(Admit)
        /\ WF_vars(RejectUnderfunded)
        /\ WF_vars(Commit)
        /\ WF_vars(Replay)
        /\ WF_vars(Settle)
        /\ WF_vars(RejectDrift)

TypeOK ==
  /\ phase \in {"Preflight", "Admission", "Commit", "Replay", "Settlement", "Done", "Rejected"}
  /\ proofCost \in Nat
  /\ commitCost \in Nat
  /\ replayCost \in Nat
  /\ proofPost \in Schedules \cup {<<>>}
  /\ commitPost \in Schedules \cup {<<>>}
  /\ replayPost \in Schedules \cup {<<>>}
  /\ supply \in Nat
  /\ preflightSchedule \in Schedules
  /\ commitSchedule \in Schedules
  /\ replaySchedule \in Schedules
  /\ completed \in BOOLEAN
  /\ executionCount \in Nat
  /\ candidateCheckpointed \in BOOLEAN

AdmittedProofCompleted == phase \in {"Commit", "Replay", "Settlement", "Done"} => completed

AdmissionRequiresCompletedProof ==
  phase \in {"Admission", "Commit", "Replay", "Settlement", "Done"} => completed

AdmittedCostIsFunded ==
  phase \in {"Commit", "Replay", "Settlement", "Done"} => proofCost + Fee <= InitialSupply

PreflightCommitReplayAgree ==
  phase = "Done" =>
    /\ IF UseStructuralProof THEN commitCost <= proofCost ELSE proofCost = commitCost
    /\ commitCost = replayCost
    /\ IF UseStructuralProof THEN TRUE ELSE proofPost = commitPost
    /\ commitPost = replayPost

EvidenceMatchesCommit ==
  phase \in {"Replay", "Settlement", "Done"} =>
    /\ IF UseStructuralProof THEN commitCost <= proofCost ELSE proofCost = commitCost
    /\ IF UseStructuralProof THEN TRUE ELSE proofPost = commitPost

CommitMatchesReplay == phase \in {"Settlement", "Done"} =>
  /\ commitCost = replayCost
  /\ commitPost = replayPost

SettlementIsExact == phase = "Done" => supply + commitCost + Fee = InitialSupply

DependentAdmissionExecutesOnce ==
  ~UseStructuralProof /\ phase \in {"Replay", "Settlement", "Done"} => executionCount = 1

RejectedExecutionIsNotPublished ==
  phase \in {"Preflight", "Admission", "Rejected"} => commitPost = <<>>

PhysicalRejectionCreatesNoCheckpoint ==
  phase = "Rejected" => ~candidateCheckpointed

PublishedCandidateHasCheckpoint ==
  phase \in {"Replay", "Settlement", "Done"} => candidateCheckpointed

CommittedExecutionIsFinite ==
  phase \in {"Replay", "Settlement", "Done"} => commitCost <= Capacity

ReplayUsesFiniteCertificate ==
  phase \in {"Settlement", "Done"} => replayCost <= Capacity

ScheduleIndependentCost ==
  \A left, right \in Schedules : ScheduleCost(left) = ScheduleCost(right)

ScheduleIndependentSemanticPost ==
  \A left, right \in Schedules : EventSet(left) = EventSet(right)

EventuallyDoneOrRejected == <>(phase \in {"Done", "Rejected"})

=============================================================================
