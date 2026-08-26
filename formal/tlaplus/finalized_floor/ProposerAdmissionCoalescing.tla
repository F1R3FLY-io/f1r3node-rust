-------------------- MODULE ProposerAdmissionCoalescing --------------------
EXTENDS Naturals

CONSTANTS
  \* @type: Str;
  Defect,
  \* @type: Int;
  MaxHeight,
  \* @type: Int;
  MaxRound,
  \* @type: Int;
  MaxIngress,
  \* Models the shard's configured heartbeat authority to create an empty block.
  \* @type: Bool;
  HeartbeatEmptyCapability

Defects == {
  "None",
  "AmbientAsyncEmpty",
  "DropPendingWake",
  "AcceptStaleRecovery"
}

RequestKinds == {"Manual", "PendingDeploy", "FinalityRecovery"}
AllRequestKinds == RequestKinds \union {"None"}
RequestSources == {"Ingress", "FreshRetry", "Coalesced"}
GateStates == {"Idle", "Active", "ActiveDirty"}
NoRound == MaxRound + 1

ASSUME /\ Defect \in Defects
       /\ MaxHeight \in Nat \ {0}
       /\ MaxRound \in Nat \ {0}
       /\ MaxIngress \in Nat \ {0}
       /\ HeartbeatEmptyCapability \in BOOLEAN

\* @typeAlias: request = { kind: Str, source: Str, capturedFloor: Int, capturedHeight: Int, capturedRound: Int, allowEmpty: Bool, forced: Bool };
module_typedefs == TRUE

\* @type: Set($request);
RequestType ==
  [kind: AllRequestKinds,
   source: RequestSources,
   capturedFloor: 0..MaxHeight,
   capturedHeight: 0..MaxHeight,
   capturedRound: 0..MaxRound,
   allowEmpty: BOOLEAN,
   forced: BOOLEAN]

\* @type: $request;
NoRequest ==
  [kind |-> "None",
   source |-> "Ingress",
   capturedFloor |-> 0,
   capturedHeight |-> 0,
   capturedRound |-> 0,
   allowEmpty |-> FALSE,
   forced |-> FALSE]

\* @type: $request;
ManualRequest ==
  [NoRequest EXCEPT !.kind = "Manual"]

\* @type: $request;
PendingDeployRequest ==
  [NoRequest EXCEPT !.kind = "PendingDeploy"]

\* @type: (Str, Int, Int, Int) => $request;
RecoveryRequest(requestSource, currentFloor, currentHeight, currentRound) ==
  [kind |-> "FinalityRecovery",
   source |-> requestSource,
   capturedFloor |-> currentFloor,
   capturedHeight |-> currentHeight,
   capturedRound |-> currentRound,
   allowEmpty |-> HeartbeatEmptyCapability,
   forced |-> FALSE]

\* @type: (Int, Int, Int) => $request;
CoalescedFollowUp(currentFloor, currentHeight, currentRound) ==
  [kind |-> "PendingDeploy",
   source |-> "Coalesced",
   capturedFloor |-> currentFloor,
   capturedHeight |-> currentHeight,
   capturedRound |-> currentRound,
   allowEmpty |-> Defect = "AmbientAsyncEmpty",
   forced |-> TRUE]

VARIABLES
  \* @type: Str;
  gate,
  \* @type: $request;
  activeRequest,
  \* @type: Bool;
  pendingWakeOwed,
  \* A later heartbeat tick owns this retry obligation. The coalescer does not.
  \* @type: Bool;
  recoveryRetry,
  \* @type: Int;
  floorId,
  \* @type: Int;
  floorHeight,
  \* @type: Int;
  height,
  \* @type: Int;
  round,
  \* @type: Int;
  selectedRound,
  \* @type: Int;
  manualIngress,
  \* @type: Int;
  pendingIngress,
  \* @type: Int;
  recoveryIngress,
  \* @type: Int;
  dirtyEpochs,
  \* @type: Int;
  forcedFollowUps,
  \* @type: Int;
  recoveryRetryEpochs,
  \* @type: Int;
  recoveryRetryServices,
  \* @type: Int;
  emittedEmpty,
  \* @type: Bool;
  invalidEmptyEmitted,
  \* @type: Bool;
  staleRecoveryAccepted

vars ==
  <<gate, activeRequest, pendingWakeOwed, recoveryRetry,
    floorId, floorHeight, height, round, selectedRound,
    manualIngress, pendingIngress, recoveryIngress,
    dirtyEpochs, forcedFollowUps,
    recoveryRetryEpochs, recoveryRetryServices,
    emittedEmpty, invalidEmptyEmitted, staleRecoveryAccepted>>

\* @type: Seq(Int);
SnapshotVars == <<floorId, floorHeight, height, round, selectedRound>>
\* @type: Seq(Int);
IngressVars == <<manualIngress, pendingIngress, recoveryIngress>>
\* @type: <<Int, Bool, Bool>>;
EvidenceVars ==
  <<emittedEmpty, invalidEmptyEmitted, staleRecoveryAccepted>>

\* @type: $request => Bool;
RecoveryStillValid(request) ==
  /\ request.kind = "FinalityRecovery"
  /\ request.capturedFloor = floorId
  /\ request.capturedHeight = floorHeight
  /\ selectedRound = request.capturedRound

Init ==
  /\ gate = "Idle"
  /\ activeRequest = NoRequest
  /\ pendingWakeOwed = FALSE
  /\ recoveryRetry = FALSE
  /\ floorId = 0
  /\ floorHeight = 0
  /\ height = 0
  /\ round = 0
  /\ selectedRound = NoRound
  /\ manualIngress = 0
  /\ pendingIngress = 0
  /\ recoveryIngress = 0
  /\ dirtyEpochs = 0
  /\ forcedFollowUps = 0
  /\ recoveryRetryEpochs = 0
  /\ recoveryRetryServices = 0
  /\ emittedEmpty = 0
  /\ invalidEmptyEmitted = FALSE
  /\ staleRecoveryAccepted = FALSE

\* @type: $request => Bool;
Admit(request) ==
  /\ gate' = "Active"
  /\ activeRequest' = request
  /\ pendingWakeOwed' = FALSE
  /\ recoveryRetry' = recoveryRetry
  /\ dirtyEpochs' = dirtyEpochs
  /\ forcedFollowUps' = forcedFollowUps
  /\ recoveryRetryEpochs' = recoveryRetryEpochs
  /\ recoveryRetryServices' = recoveryRetryServices

ManualCollision ==
  /\ UNCHANGED <<gate, activeRequest, pendingWakeOwed, recoveryRetry>>
  /\ UNCHANGED
       <<dirtyEpochs, forcedFollowUps,
         recoveryRetryEpochs, recoveryRetryServices>>

PendingCollision ==
  /\ activeRequest' = activeRequest
  /\ pendingWakeOwed' = TRUE
  /\ recoveryRetry' = recoveryRetry
  /\ forcedFollowUps' = forcedFollowUps
  /\ recoveryRetryEpochs' = recoveryRetryEpochs
  /\ recoveryRetryServices' = recoveryRetryServices
  /\ IF Defect = "DropPendingWake"
        THEN /\ gate' = gate
             /\ dirtyEpochs' = dirtyEpochs
        ELSE /\ gate' = "ActiveDirty"
             /\ dirtyEpochs' =
                  IF gate = "Active" THEN dirtyEpochs + 1 ELSE dirtyEpochs

RecoveryCollision ==
  /\ UNCHANGED <<gate, activeRequest, pendingWakeOwed>>
  /\ UNCHANGED <<dirtyEpochs, forcedFollowUps, recoveryRetryServices>>
  /\ recoveryRetry' = TRUE
  /\ recoveryRetryEpochs' =
       IF recoveryRetry THEN recoveryRetryEpochs ELSE recoveryRetryEpochs + 1

RequestManual ==
  /\ manualIngress < MaxIngress
  /\ manualIngress' = manualIngress + 1
  /\ UNCHANGED <<pendingIngress, recoveryIngress>>
  /\ IF gate = "Idle"
        THEN Admit(ManualRequest)
        ELSE ManualCollision
  /\ UNCHANGED SnapshotVars
  /\ UNCHANGED EvidenceVars

RequestPendingDeploy ==
  /\ pendingIngress < MaxIngress
  /\ pendingIngress' = pendingIngress + 1
  /\ UNCHANGED <<manualIngress, recoveryIngress>>
  /\ IF gate = "Idle"
        THEN Admit(PendingDeployRequest)
        ELSE PendingCollision
  /\ UNCHANGED SnapshotVars
  /\ UNCHANGED EvidenceVars

RequestFinalityRecovery ==
  /\ recoveryIngress < MaxIngress
  /\ selectedRound = round
  /\ recoveryIngress' = recoveryIngress + 1
  /\ UNCHANGED <<manualIngress, pendingIngress>>
  /\ IF gate = "Idle"
        THEN Admit(RecoveryRequest("Ingress", floorId, floorHeight, round))
        ELSE RecoveryCollision
  /\ UNCHANGED SnapshotVars
  /\ UNCHANGED EvidenceVars

SelectRecovery ==
  /\ selectedRound # round
  /\ selectedRound' = round
  /\ UNCHANGED
       <<gate, activeRequest, pendingWakeOwed, recoveryRetry,
         floorId, floorHeight, height, round,
         manualIngress, pendingIngress, recoveryIngress,
         dirtyEpochs, forcedFollowUps,
         recoveryRetryEpochs, recoveryRetryServices,
         emittedEmpty, invalidEmptyEmitted, staleRecoveryAccepted>>

AdvanceHeight ==
  /\ height < MaxHeight
  /\ height' = height + 1
  /\ UNCHANGED
       <<gate, activeRequest, pendingWakeOwed, recoveryRetry,
         floorId, floorHeight, round, selectedRound,
         manualIngress, pendingIngress, recoveryIngress,
         dirtyEpochs, forcedFollowUps,
         recoveryRetryEpochs, recoveryRetryServices,
         emittedEmpty, invalidEmptyEmitted, staleRecoveryAccepted>>

AdvanceFloor ==
  /\ floorHeight < height
  /\ floorId < MaxHeight
  /\ floorId' = floorId + 1
  /\ floorHeight' = floorHeight + 1
  /\ UNCHANGED
       <<gate, activeRequest, pendingWakeOwed, recoveryRetry,
         height, round, selectedRound,
         manualIngress, pendingIngress, recoveryIngress,
         dirtyEpochs, forcedFollowUps,
         recoveryRetryEpochs, recoveryRetryServices,
         emittedEmpty, invalidEmptyEmitted, staleRecoveryAccepted>>

AdvanceRound ==
  /\ round < MaxRound
  /\ activeRequest.kind # "FinalityRecovery"
  /\ round' = round + 1
  /\ selectedRound' = NoRound
  /\ UNCHANGED
       <<gate, activeRequest, pendingWakeOwed, recoveryRetry,
         floorId, floorHeight, height,
         manualIngress, pendingIngress, recoveryIngress,
         dirtyEpochs, forcedFollowUps,
         recoveryRetryEpochs, recoveryRetryServices,
         emittedEmpty, invalidEmptyEmitted, staleRecoveryAccepted>>

Execute ==
  /\ gate # "Idle"
  /\ LET staleRecovery ==
            activeRequest.kind = "FinalityRecovery"
              /\ ~RecoveryStillValid(activeRequest)
         acceptsStale == staleRecovery /\ Defect = "AcceptStaleRecovery"
         accepted == ~staleRecovery \/ acceptsStale
         makesEmpty == accepted /\ activeRequest.allowEmpty
     IN
       /\ emittedEmpty' = emittedEmpty + IF makesEmpty THEN 1 ELSE 0
       /\ invalidEmptyEmitted' =
            (invalidEmptyEmitted
              \/ (makesEmpty /\ ~RecoveryStillValid(activeRequest)))
       /\ staleRecoveryAccepted' =
            (staleRecoveryAccepted \/ acceptsStale)
  /\ IF gate = "ActiveDirty"
        THEN /\ gate' = "Active"
             /\ activeRequest' =
                  CoalescedFollowUp(floorId, floorHeight, round)
             /\ pendingWakeOwed' = FALSE
             /\ dirtyEpochs' = dirtyEpochs
             /\ forcedFollowUps' = forcedFollowUps + 1
        ELSE /\ gate' = "Idle"
             /\ activeRequest' = NoRequest
             /\ pendingWakeOwed' = pendingWakeOwed
             /\ dirtyEpochs' = dirtyEpochs
             /\ forcedFollowUps' = forcedFollowUps
  /\ UNCHANGED recoveryRetry
  /\ UNCHANGED SnapshotVars
  /\ UNCHANGED IngressVars
  /\ UNCHANGED <<recoveryRetryEpochs, recoveryRetryServices>>

\* This action is a fresh external heartbeat invocation after a Busy response.
\* Engine unavailability may clear the coalescer latch; the deploy remains in
\* persistent storage, whose retry semantics are modeled by
\* PendingDeployHeartbeatComposition.
ServiceRecoveryRetry ==
  /\ gate = "Idle"
  /\ recoveryRetry
  /\ recoveryRetry' = FALSE
  /\ recoveryRetryServices' = recoveryRetryServices + 1
  /\ IF selectedRound = round
        THEN /\ gate' = "Active"
             /\ activeRequest' =
                  RecoveryRequest("FreshRetry", floorId, floorHeight, round)
             /\ pendingWakeOwed' = FALSE
        ELSE /\ gate' = "Idle"
             /\ activeRequest' = NoRequest
             /\ pendingWakeOwed' = FALSE
  /\ UNCHANGED SnapshotVars
  /\ UNCHANGED IngressVars
  /\ UNCHANGED <<dirtyEpochs, forcedFollowUps, recoveryRetryEpochs>>
  /\ UNCHANGED EvidenceVars

Next ==
  \/ RequestManual
  \/ RequestPendingDeploy
  \/ RequestFinalityRecovery
  \/ SelectRecovery
  \/ AdvanceHeight
  \/ AdvanceFloor
  \/ AdvanceRound
  \/ Execute
  \/ ServiceRecoveryRetry

Spec ==
  /\ Init
  /\ [][Next]_vars
  /\ WF_vars(Execute)
  /\ WF_vars(ServiceRecoveryRetry)

TypeOK ==
  /\ gate \in GateStates
  /\ activeRequest \in RequestType
  /\ pendingWakeOwed \in BOOLEAN
  /\ recoveryRetry \in BOOLEAN
  /\ height \in 0..MaxHeight
  /\ floorId \in 0..MaxHeight
  /\ floorHeight \in 0..height
  /\ round \in 0..MaxRound
  /\ selectedRound \in (0..MaxRound) \union {NoRound}
  /\ manualIngress \in 0..MaxIngress
  /\ pendingIngress \in 0..MaxIngress
  /\ recoveryIngress \in 0..MaxIngress
  /\ dirtyEpochs \in Nat
  /\ forcedFollowUps \in Nat
  /\ recoveryRetryEpochs \in Nat
  /\ recoveryRetryServices \in Nat
  /\ emittedEmpty \in Nat
  /\ invalidEmptyEmitted \in BOOLEAN
  /\ staleRecoveryAccepted \in BOOLEAN

Inv_IdleHasNoActiveRequest ==
  (gate = "Idle") <=> (activeRequest = NoRequest)

Inv_PendingWakeLatched ==
  pendingWakeOwed => gate = "ActiveDirty"

Inv_ExactlyOneFollowUpPerDirtyEpoch ==
  /\ forcedFollowUps <= dirtyEpochs
  /\ dirtyEpochs <= forcedFollowUps + 1
  /\ ((dirtyEpochs = forcedFollowUps + 1) <=> (gate = "ActiveDirty"))

Inv_ForcedFollowUpIsNonEmpty ==
  activeRequest.forced =>
    /\ activeRequest.kind = "PendingDeploy"
    /\ activeRequest.source = "Coalesced"
    /\ ~activeRequest.allowEmpty

Inv_EmptyAuthorityIsRecoveryOnly ==
  activeRequest.allowEmpty =>
    activeRequest.kind = "FinalityRecovery"

Inv_RecoveryRetryIsCoalesced ==
  /\ recoveryRetryServices <= recoveryRetryEpochs
  /\ recoveryRetryEpochs <= recoveryRetryServices + 1
  /\ recoveryRetry = (recoveryRetryEpochs = recoveryRetryServices + 1)

Inv_StaleRecoveryPermitRejected == ~staleRecoveryAccepted

Inv_NoEmptyWithoutValidSelectedRecovery == ~invalidEmptyEmitted

Live_GateEventuallyQuiescent == []<>(gate = "Idle" /\ ~recoveryRetry)

Live_DirtyEpochEventuallyServiced ==
  [](gate = "ActiveDirty" => <> (forcedFollowUps = dirtyEpochs))

Live_RecoveryCollisionEventuallyRetriedOrDiscarded ==
  [](recoveryRetry => <> ~recoveryRetry)

=============================================================================
