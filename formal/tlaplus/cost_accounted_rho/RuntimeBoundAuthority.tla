------------------------ MODULE RuntimeBoundAuthority ------------------------
EXTENDS Naturals, TLC

CONSTANTS Slot, AlternateSlot, InitialEnvelopeSupply, CreatorDemand,
          TriggerDemand, CandidateStackDepth, RejectBoundBeforeExecution,
          ReadCandidateSupply, ReplayRebindsSlot

ASSUME /\ Slot # AlternateSlot
       /\ InitialEnvelopeSupply \in Nat
       /\ CreatorDemand \in Nat
       /\ TriggerDemand \in Nat \ {0}
       /\ CandidateStackDepth \in Nat \ {0}
       /\ RejectBoundBeforeExecution \in BOOLEAN
       /\ ReadCandidateSupply \in BOOLEAN
       /\ ReplayRebindsSlot \in BOOLEAN

VARIABLES phase, creatorAdmitted, observedCreatorSupply, continuationPersisted,
          stackDepth, eventSlot, settlementSlot, replaySlot, charged,
          checkpointed

vars == <<phase, creatorAdmitted, observedCreatorSupply,
          continuationPersisted, stackDepth, eventSlot, settlementSlot,
          replaySlot, charged, checkpointed>>

NoSlot == "NoSlot"

Init ==
  /\ phase = "CreatorPreflight"
  /\ creatorAdmitted = FALSE
  /\ observedCreatorSupply = 0
  /\ continuationPersisted = FALSE
  /\ stackDepth = 0
  /\ eventSlot = NoSlot
  /\ settlementSlot = NoSlot
  /\ replaySlot = NoSlot
  /\ charged = 0
  /\ checkpointed = FALSE

CreatorPreflight ==
  /\ phase = "CreatorPreflight"
  /\ observedCreatorSupply' =
       IF ReadCandidateSupply
       THEN InitialEnvelopeSupply + CandidateStackDepth
       ELSE InitialEnvelopeSupply
  /\ IF RejectBoundBeforeExecution
        THEN /\ creatorAdmitted' = FALSE
             /\ phase' = "RejectedBound"
        ELSE IF CreatorDemand <=
                    IF ReadCandidateSupply
                    THEN InitialEnvelopeSupply + CandidateStackDepth
                    ELSE InitialEnvelopeSupply
             THEN /\ creatorAdmitted' = TRUE
                  /\ phase' = "ExecuteCreator"
             ELSE /\ creatorAdmitted' = FALSE
                  /\ phase' = "RejectedFunding"
  /\ UNCHANGED <<continuationPersisted, stackDepth, eventSlot,
                  settlementSlot, replaySlot, charged, checkpointed>>

ExecuteCreator ==
  /\ phase = "ExecuteCreator"
  /\ creatorAdmitted
  /\ continuationPersisted' = TRUE
  /\ stackDepth' = CandidateStackDepth
  /\ checkpointed' = TRUE
  /\ phase' = "Trigger"
  /\ UNCHANGED <<creatorAdmitted, observedCreatorSupply, eventSlot,
                  settlementSlot, replaySlot, charged>>

Trigger ==
  /\ phase = "Trigger"
  /\ continuationPersisted
  /\ TriggerDemand <= stackDepth
  /\ eventSlot' = Slot
  /\ phase' = "Settle"
  /\ UNCHANGED <<creatorAdmitted, observedCreatorSupply,
                  continuationPersisted, stackDepth, settlementSlot,
                  replaySlot, charged, checkpointed>>

Settle ==
  /\ phase = "Settle"
  /\ eventSlot = Slot
  /\ stackDepth' = stackDepth - TriggerDemand
  /\ settlementSlot' = eventSlot
  /\ charged' = TriggerDemand
  /\ phase' = "Replay"
  /\ UNCHANGED <<creatorAdmitted, observedCreatorSupply,
                  continuationPersisted, eventSlot, replaySlot,
                  checkpointed>>

Replay ==
  /\ phase = "Replay"
  /\ replaySlot' = IF ReplayRebindsSlot THEN AlternateSlot ELSE settlementSlot
  /\ phase' = "Done"
  /\ UNCHANGED <<creatorAdmitted, observedCreatorSupply,
                  continuationPersisted, stackDepth, eventSlot,
                  settlementSlot, charged, checkpointed>>

Next == CreatorPreflight \/ ExecuteCreator \/ Trigger \/ Settle \/ Replay

Spec == /\ Init
        /\ [][Next]_vars
        /\ WF_vars(CreatorPreflight)
        /\ WF_vars(ExecuteCreator)
        /\ WF_vars(Trigger)
        /\ WF_vars(Settle)
        /\ WF_vars(Replay)

TypeOK ==
  /\ phase \in {"CreatorPreflight", "ExecuteCreator", "Trigger", "Settle",
                  "Replay", "Done", "RejectedBound", "RejectedFunding"}
  /\ creatorAdmitted \in BOOLEAN
  /\ observedCreatorSupply \in Nat
  /\ continuationPersisted \in BOOLEAN
  /\ stackDepth \in Nat
  /\ eventSlot \in {NoSlot, Slot, AlternateSlot}
  /\ settlementSlot \in {NoSlot, Slot, AlternateSlot}
  /\ replaySlot \in {NoSlot, Slot, AlternateSlot}
  /\ charged \in Nat
  /\ checkpointed \in BOOLEAN

BoundAuthorityDeferred == phase # "RejectedBound"

CandidateStackCannotFundCreator ==
  creatorAdmitted => CreatorDemand <= InitialEnvelopeSupply

CommittedCreatorPersistsResolvedAuthority ==
  phase \in {"Trigger", "Settle", "Replay", "Done"} =>
    /\ creatorAdmitted
    /\ continuationPersisted
    /\ checkpointed

TriggeredEventUsesPersistedSlot ==
  phase \in {"Settle", "Replay", "Done"} => eventSlot = Slot

SettlementDebitsExactStackPrefix ==
  phase \in {"Replay", "Done"} =>
    /\ settlementSlot = Slot
    /\ charged = TriggerDemand
    /\ stackDepth + TriggerDemand = CandidateStackDepth

ReplayPreservesResolvedSlot ==
  phase = "Done" => replaySlot = settlementSlot

EventuallyDoneOrFundingRejected ==
  <>(phase \in {"Done", "RejectedFunding"})

=============================================================================
