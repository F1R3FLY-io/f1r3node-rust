---------------------- MODULE SettledTicketTransaction ----------------------
EXTENDS Naturals, FiniteSets

CONSTANTS
    \* @type: Set(Str);
    Hashes,
    \* @type: Set(Str);
    Workers,
    \* @type: Int;
    MaxBudget,
    \* @type: Bool;
    ConsumeOnClaim,
    \* @type: Bool;
    DropEvidenceOnError,
    \* @type: Bool;
    ValidateDuplicate,
    \* @type: Bool;
    KeepFailedReservation,
    \* @type: Bool;
    RestoreAfterCommit,
    \* @type: Bool;
    ProofAuthentic,
    \* @type: Bool;
    RebuildBudgetOnRestart

Phases == {"waiting", "claimed", "committed"}

VARIABLES
    \* @type: Str -> Str;
    phase,
    \* @type: Str -> Set(Str);
    owners,
    \* @type: Str -> Bool;
    durableEvidence,
    \* @type: Str -> Bool;
    bufferEdge,
    \* @type: Str -> Bool;
    reserved,
    \* @type: Str -> Bool;
    durableDag,
    \* @type: Str -> Bool;
    cleanupPending,
    \* @type: Str -> Bool;
    ordinaryValidation,
    \* @type: Int;
    budgetCounter,
    \* @type: Int;
    duplicateCount,
    \* @type: Bool;
    restartAvailable

vars ==
    <<phase, owners, durableEvidence, bufferEdge, reserved, durableDag,
      cleanupPending, ordinaryValidation, budgetCounter, duplicateCount,
      restartAvailable>>

Claimed == {hash \in Hashes : phase[hash] = "claimed"}
Committed == {hash \in Hashes : phase[hash] = "committed"}
Reserved == {hash \in Hashes : reserved[hash]}

Init ==
    /\ phase = [hash \in Hashes |-> "waiting"]
    /\ owners = [hash \in Hashes |-> {}]
    /\ durableEvidence = [hash \in Hashes |-> TRUE]
    /\ bufferEdge = [hash \in Hashes |-> TRUE]
    /\ reserved = [hash \in Hashes |-> FALSE]
    /\ durableDag = [hash \in Hashes |-> FALSE]
    /\ cleanupPending = [hash \in Hashes |-> FALSE]
    /\ ordinaryValidation = [hash \in Hashes |-> FALSE]
    /\ budgetCounter = 0
    /\ duplicateCount = 2
    /\ restartAvailable = TRUE

ClaimOne(hash, worker) ==
    /\ phase[hash] = "waiting"
    /\ durableEvidence[hash]
    /\ IF ConsumeOnClaim
       THEN /\ phase' = [phase EXCEPT ![hash] = "committed"]
            /\ owners' = owners
       ELSE /\ phase' = [phase EXCEPT ![hash] = "claimed"]
            /\ owners' = [owners EXCEPT ![hash] = {worker}]
    /\ UNCHANGED
        <<durableEvidence, bufferEdge, reserved, durableDag, cleanupPending,
          ordinaryValidation, budgetCounter, duplicateCount, restartAvailable>>

ReserveOne(hash, worker) ==
    /\ phase[hash] = "claimed"
    /\ owners[hash] = {worker}
    /\ ~reserved[hash]
    /\ budgetCounter < MaxBudget
    /\ reserved' = [reserved EXCEPT ![hash] = TRUE]
    /\ budgetCounter' = budgetCounter + 1
    /\ UNCHANGED
        <<phase, owners, durableEvidence, bufferEdge, durableDag,
          cleanupPending, ordinaryValidation, duplicateCount,
          restartAvailable>>

ReleaseBeforeCommit(hash, worker) ==
    /\ phase[hash] = "claimed"
    /\ owners[hash] = {worker}
    /\ phase' = [phase EXCEPT ![hash] = "waiting"]
    /\ owners' = [owners EXCEPT ![hash] = {}]
    /\ durableEvidence' =
        [durableEvidence EXCEPT ![hash] = IF DropEvidenceOnError THEN FALSE ELSE @]
    /\ bufferEdge' =
        [bufferEdge EXCEPT ![hash] = IF DropEvidenceOnError THEN FALSE ELSE @]
    /\ reserved' = [reserved EXCEPT ![hash] = FALSE]
    /\ budgetCounter' =
        IF reserved[hash] /\ ~KeepFailedReservation
        THEN budgetCounter - 1
        ELSE budgetCounter
    /\ UNCHANGED
        <<durableDag, cleanupPending, ordinaryValidation, duplicateCount,
          restartAvailable>>

CommitOne(hash, worker) ==
    /\ phase[hash] = "claimed"
    /\ owners[hash] = {worker}
    /\ reserved[hash]
    /\ phase' = [phase EXCEPT ![hash] = "committed"]
    /\ owners' = [owners EXCEPT ![hash] = {}]
    /\ reserved' = [reserved EXCEPT ![hash] = FALSE]
    /\ durableDag' = [durableDag EXCEPT ![hash] = TRUE]
    /\ cleanupPending' = [cleanupPending EXCEPT ![hash] = TRUE]
    /\ UNCHANGED
        <<durableEvidence, bufferEdge, ordinaryValidation, budgetCounter,
          duplicateCount, restartAvailable>>

DeliverDuplicate(hash, worker) ==
    /\ phase[hash] \in {"claimed", "committed"}
    /\ worker \in Workers
    /\ duplicateCount > 0
    /\ duplicateCount' = duplicateCount - 1
    /\ ordinaryValidation' =
        [ordinaryValidation EXCEPT ![hash] = IF ValidateDuplicate THEN TRUE ELSE @]
    /\ UNCHANGED
        <<phase, owners, durableEvidence, bufferEdge, reserved, durableDag,
          cleanupPending, budgetCounter, restartAvailable>>

FinishCleanup(hash) ==
    /\ phase[hash] = "committed"
    /\ cleanupPending[hash]
    /\ cleanupPending' = [cleanupPending EXCEPT ![hash] = FALSE]
    /\ durableEvidence' = [durableEvidence EXCEPT ![hash] = FALSE]
    /\ bufferEdge' = [bufferEdge EXCEPT ![hash] = FALSE]
    /\ UNCHANGED
        <<phase, owners, reserved, durableDag, ordinaryValidation,
          budgetCounter, duplicateCount, restartAvailable>>

PostCommitFailure(hash) ==
    /\ phase[hash] = "committed"
    /\ cleanupPending[hash]
    /\ RestoreAfterCommit
    /\ phase' = [phase EXCEPT ![hash] = "waiting"]
    /\ durableEvidence' = [durableEvidence EXCEPT ![hash] = TRUE]
    /\ bufferEdge' = [bufferEdge EXCEPT ![hash] = TRUE]
    /\ cleanupPending' = [cleanupPending EXCEPT ![hash] = FALSE]
    /\ UNCHANGED
        <<owners, reserved, durableDag, ordinaryValidation, budgetCounter,
          duplicateCount, restartAvailable>>

Restart ==
    /\ restartAvailable
    /\ phase' =
        [hash \in Hashes |->
            IF durableDag[hash] THEN "committed" ELSE "waiting"]
    /\ owners' = [hash \in Hashes |-> {}]
    /\ durableEvidence' =
        [hash \in Hashes |-> bufferEdge[hash]]
    /\ reserved' = [hash \in Hashes |-> FALSE]
    /\ cleanupPending' =
        [hash \in Hashes |-> durableDag[hash] /\ bufferEdge[hash]]
    /\ budgetCounter' =
        IF RebuildBudgetOnRestart
        THEN Cardinality({hash \in Hashes : durableDag[hash]})
        ELSE 0
    /\ restartAvailable' = FALSE
    /\ UNCHANGED
        <<bufferEdge, durableDag, ordinaryValidation, duplicateCount>>

Claim == \E hash \in Hashes, worker \in Workers : ClaimOne(hash, worker)
Reserve == \E hash \in Hashes, worker \in Workers : ReserveOne(hash, worker)
Release == \E hash \in Hashes, worker \in Workers : ReleaseBeforeCommit(hash, worker)
Commit == \E hash \in Hashes, worker \in Workers : CommitOne(hash, worker)
Duplicate == \E hash \in Hashes, worker \in Workers : DeliverDuplicate(hash, worker)
Cleanup == \E hash \in Hashes : FinishCleanup(hash)
CleanupFailure == \E hash \in Hashes : PostCommitFailure(hash)

Next == Claim \/ Reserve \/ Release \/ Commit \/ Duplicate \/ Cleanup
        \/ CleanupFailure \/ Restart

Spec == Init /\ [][Next]_vars

LiveSpec == Spec /\ WF_vars(Cleanup)

TypeOK ==
    /\ phase \in [Hashes -> Phases]
    /\ owners \in [Hashes -> SUBSET Workers]
    /\ durableEvidence \in [Hashes -> BOOLEAN]
    /\ bufferEdge \in [Hashes -> BOOLEAN]
    /\ reserved \in [Hashes -> BOOLEAN]
    /\ durableDag \in [Hashes -> BOOLEAN]
    /\ cleanupPending \in [Hashes -> BOOLEAN]
    /\ ordinaryValidation \in [Hashes -> BOOLEAN]
    /\ budgetCounter \in Nat
    /\ duplicateCount \in Nat
    /\ restartAvailable \in BOOLEAN

Inv_ExclusiveClaim ==
    \A hash \in Hashes : Cardinality(owners[hash]) <= 1

Inv_OwnerMatchesClaim ==
    \A hash \in Hashes : (phase[hash] = "claimed") <=> (owners[hash] # {})

Inv_PrecommitFailureRetainsEvidence ==
    \A hash \in Hashes : ~durableDag[hash] => durableEvidence[hash] /\ bufferEdge[hash]

Inv_CommitImpliesDurability ==
    \A hash \in Hashes : phase[hash] = "committed" => durableDag[hash]

Inv_DurableCommitIsPermanent ==
    \A hash \in Hashes : durableDag[hash] => phase[hash] = "committed"

Inv_DuplicateNeverValidates ==
    \A hash \in Hashes : ~ordinaryValidation[hash]

Inv_DurableProofAuthentic ==
    \A hash \in Hashes : durableDag[hash] => ProofAuthentic

Inv_BufferRemovalFollowsDagCommit ==
    \A hash \in Hashes : ~bufferEdge[hash] => durableDag[hash]

Inv_CleanupPendingMatchesDrift ==
    \A hash \in Hashes : cleanupPending[hash] <=> durableDag[hash] /\ bufferEdge[hash]

Inv_EvidenceMatchesBuffer ==
    \A hash \in Hashes : durableEvidence[hash] = bufferEdge[hash]

Inv_BudgetMatchesOwnership ==
    budgetCounter = Cardinality(Committed) + Cardinality(Reserved)

Inv_BudgetIsBounded == budgetCounter <= MaxBudget

EventuallyCleansCommittedEdges ==
    \A hash \in Hashes : durableDag[hash] ~> ~bufferEdge[hash]

=============================================================================
