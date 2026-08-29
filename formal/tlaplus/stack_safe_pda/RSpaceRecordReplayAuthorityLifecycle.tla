-------------- MODULE RSpaceRecordReplayAuthorityLifecycle --------------
EXTENDS Integers, Naturals, TLC

(***************************************************************************
Lifecycle refinement for recording and replaying the same RSpace transition
through one shared atomic checkpoint authority.

The recorder starts at B and publishes T.  The replayer is then required to
reactivate the known base B, freshly prepare B -> T at B's new generation,
and only then publish T.  A replay attempted directly after the recorder's
publication is stale even though its desired target is already T: authority
is the exact (root, generation) pair, not target-root equality.

The control trace deliberately attempts the forbidden skip-reset transition
and records its rejection without authority mutation before proceeding with
the valid reset/prepare/replay lifecycle.  SkipResetAcceptanceMutantSpec is
the negative control: it accepts that stale replay and must violate
ReplayCommitRequiresReactivation.
***************************************************************************)

Roots == {"B", "T"}
Phases == {"initial", "played", "skip-rejected", "reset", "prepared", "complete"}
HistoryKeys == {"h-B", "h-T"}
ColdKeys == {"c-B", "c-T"}
Values == {"v-B", "v-T"}

RootHistory(root) == IF root = "B" THEN {"h-B"} ELSE {"h-T"}
RootCold(root) == IF root = "B" THEN {"c-B"} ELSE {"c-T"}
Keys(objects) == {pair[1] : pair \in objects}
ContentImmutable(objects) ==
    \A left, right \in objects:
        left[1] = right[1] => left[2] = right[2]
Complete(root, history, cold) ==
    /\ RootHistory(root) \subseteq Keys(history)
    /\ RootCold(root) \subseteq Keys(cold)

VARIABLES phase, currentRoot, generation, activationCount,
          verifiedRoots, historyObjects, coldObjects,
          playHistory, playCold, playTarget,
          skipResetAttempted, skipResetAccepted,
          rejectionRoot, rejectionGeneration, rejectionActivationCount,
          rejectionVerifiedRoots, rejectionHistory, rejectionCold,
          replayResetObserved, resetRoot, resetGeneration,
          replayPrepared, replayBaseRoot, replayBaseGeneration, replayTarget,
          prepareRoot, prepareGeneration, prepareActivationCount,
          prepareVerifiedRoots, prepareHistory, prepareCold,
          replayCommitted

vars == <<phase, currentRoot, generation, activationCount,
          verifiedRoots, historyObjects, coldObjects,
          playHistory, playCold, playTarget,
          skipResetAttempted, skipResetAccepted,
          rejectionRoot, rejectionGeneration, rejectionActivationCount,
          rejectionVerifiedRoots, rejectionHistory, rejectionCold,
          replayResetObserved, resetRoot, resetGeneration,
          replayPrepared, replayBaseRoot, replayBaseGeneration, replayTarget,
          prepareRoot, prepareGeneration, prepareActivationCount,
          prepareVerifiedRoots, prepareHistory, prepareCold,
          replayCommitted>>

Init ==
    /\ phase = "initial"
    /\ currentRoot = "B"
    /\ generation = 0
    /\ activationCount = 0
    /\ verifiedRoots = {"B"}
    /\ historyObjects = {<<"h-B", "v-B">>}
    /\ coldObjects = {<<"c-B", "v-B">>}
    /\ playHistory = historyObjects
    /\ playCold = coldObjects
    /\ playTarget = "T"
    /\ skipResetAttempted = FALSE
    /\ skipResetAccepted = FALSE
    /\ rejectionRoot = currentRoot
    /\ rejectionGeneration = generation
    /\ rejectionActivationCount = activationCount
    /\ rejectionVerifiedRoots = verifiedRoots
    /\ rejectionHistory = historyObjects
    /\ rejectionCold = coldObjects
    /\ replayResetObserved = FALSE
    /\ resetRoot = currentRoot
    /\ resetGeneration = generation
    /\ replayPrepared = FALSE
    /\ replayBaseRoot = currentRoot
    /\ replayBaseGeneration = generation
    /\ replayTarget = "T"
    /\ prepareRoot = currentRoot
    /\ prepareGeneration = generation
    /\ prepareActivationCount = activationCount
    /\ prepareVerifiedRoots = verifiedRoots
    /\ prepareHistory = historyObjects
    /\ prepareCold = coldObjects
    /\ replayCommitted = FALSE

PlayCommit ==
    /\ phase = "initial"
    /\ currentRoot = "B"
    /\ generation = 0
    /\ historyObjects' = historyObjects \union {<<"h-T", "v-T">>}
    /\ coldObjects' = coldObjects \union {<<"c-T", "v-T">>}
    /\ verifiedRoots' = verifiedRoots \union {"T"}
    /\ currentRoot' = "T"
    /\ generation' = generation + 1
    /\ activationCount' = activationCount + 1
    /\ playHistory' = historyObjects'
    /\ playCold' = coldObjects'
    /\ phase' = "played"
    /\ UNCHANGED <<playTarget, skipResetAttempted, skipResetAccepted,
                    rejectionRoot, rejectionGeneration, rejectionActivationCount,
                    rejectionVerifiedRoots, rejectionHistory, rejectionCold,
                    replayResetObserved, resetRoot,
                    resetGeneration, replayPrepared, replayBaseRoot,
                    replayBaseGeneration, replayTarget, prepareRoot,
                    prepareGeneration, prepareActivationCount,
                    prepareVerifiedRoots, prepareHistory, prepareCold,
                    replayCommitted>>

RejectSkipReset ==
    /\ phase = "played"
    /\ currentRoot = "T"
    /\ skipResetAttempted' = TRUE
    /\ skipResetAccepted' = FALSE
    /\ rejectionRoot' = currentRoot
    /\ rejectionGeneration' = generation
    /\ rejectionActivationCount' = activationCount
    /\ rejectionVerifiedRoots' = verifiedRoots
    /\ rejectionHistory' = historyObjects
    /\ rejectionCold' = coldObjects
    /\ phase' = "skip-rejected"
    /\ UNCHANGED <<currentRoot, generation, activationCount, verifiedRoots,
                    historyObjects, coldObjects, playHistory, playCold,
                    playTarget, replayResetObserved, resetRoot,
                    resetGeneration, replayPrepared, replayBaseRoot,
                    replayBaseGeneration, replayTarget, prepareRoot,
                    prepareGeneration, prepareActivationCount,
                    prepareVerifiedRoots, prepareHistory, prepareCold,
                    replayCommitted>>

ReactivateReplayBase ==
    /\ phase = "skip-rejected"
    /\ "B" \in verifiedRoots
    /\ currentRoot' = "B"
    /\ generation' = generation + 1
    /\ activationCount' = activationCount + 1
    /\ replayResetObserved' = TRUE
    /\ resetRoot' = "B"
    /\ resetGeneration' = generation'
    /\ phase' = "reset"
    /\ UNCHANGED <<verifiedRoots, historyObjects, coldObjects, playHistory,
                    playCold, playTarget, skipResetAttempted,
                    skipResetAccepted, rejectionRoot, rejectionGeneration,
                    rejectionActivationCount, rejectionVerifiedRoots,
                    rejectionHistory, rejectionCold, replayPrepared,
                    replayBaseRoot, replayBaseGeneration, replayTarget,
                    prepareRoot, prepareGeneration, prepareActivationCount,
                    prepareVerifiedRoots, prepareHistory, prepareCold,
                    replayCommitted>>

FreshReplayPrepare ==
    /\ phase = "reset"
    /\ currentRoot = "B"
    /\ generation = resetGeneration
    /\ replayPrepared' = TRUE
    /\ replayBaseRoot' = currentRoot
    /\ replayBaseGeneration' = generation
    /\ replayTarget' = playTarget
    /\ prepareRoot' = currentRoot
    /\ prepareGeneration' = generation
    /\ prepareActivationCount' = activationCount
    /\ prepareVerifiedRoots' = verifiedRoots
    /\ prepareHistory' = historyObjects
    /\ prepareCold' = coldObjects
    /\ phase' = "prepared"
    /\ UNCHANGED <<currentRoot, generation, activationCount, verifiedRoots,
                    historyObjects, coldObjects, playHistory, playCold,
                    playTarget, skipResetAttempted, skipResetAccepted,
                    rejectionRoot, rejectionGeneration, rejectionActivationCount,
                    rejectionVerifiedRoots, rejectionHistory, rejectionCold,
                    replayResetObserved, resetRoot,
                    resetGeneration, replayCommitted>>

ReplayCommit ==
    /\ phase = "prepared"
    /\ replayPrepared
    /\ currentRoot = replayBaseRoot
    /\ generation = replayBaseGeneration
    /\ replayTarget = playTarget
    /\ replayTarget \in verifiedRoots
    /\ Complete(replayTarget, historyObjects, coldObjects)
    /\ currentRoot' = replayTarget
    /\ generation' = generation + 1
    /\ activationCount' = activationCount + 1
    /\ replayCommitted' = TRUE
    /\ phase' = "complete"
    /\ UNCHANGED <<verifiedRoots, historyObjects, coldObjects, playHistory,
                    playCold, playTarget, skipResetAttempted,
                    skipResetAccepted, rejectionRoot, rejectionGeneration,
                    rejectionActivationCount, rejectionVerifiedRoots,
                    rejectionHistory, rejectionCold, replayResetObserved,
                    resetRoot, resetGeneration, replayPrepared,
                    replayBaseRoot, replayBaseGeneration, replayTarget,
                    prepareRoot, prepareGeneration, prepareActivationCount,
                    prepareVerifiedRoots, prepareHistory, prepareCold>>

Done == phase = "complete" /\ UNCHANGED vars

Next == PlayCommit \/ RejectSkipReset \/ ReactivateReplayBase \/
        FreshReplayPrepare \/ ReplayCommit \/ Done
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeInvariant ==
    /\ phase \in Phases
    /\ currentRoot \in Roots
    /\ generation \in Nat
    /\ activationCount \in Nat
    /\ verifiedRoots \subseteq Roots
    /\ historyObjects \subseteq HistoryKeys \X Values
    /\ coldObjects \subseteq ColdKeys \X Values
    /\ replayBaseRoot \in Roots
    /\ replayBaseGeneration \in Nat
    /\ resetRoot \in Roots
    /\ resetGeneration \in Nat
    /\ rejectionActivationCount \in Nat
    /\ rejectionVerifiedRoots \subseteq Roots
    /\ prepareActivationCount \in Nat
    /\ prepareVerifiedRoots \subseteq Roots

CurrentRootIsVerified == currentRoot \in verifiedRoots

VerifiedRootsHaveCompleteClosure ==
    \A root \in verifiedRoots: Complete(root, historyObjects, coldObjects)

ContentKeysAreImmutable ==
    /\ ContentImmutable(historyObjects)
    /\ ContentImmutable(coldObjects)

NoUnpublishedCheckpointObjects ==
    /\ Keys(historyObjects) = UNION {RootHistory(root) : root \in verifiedRoots}
    /\ Keys(coldObjects) = UNION {RootCold(root) : root \in verifiedRoots}

GenerationAdvancesExactlyOnActivation == generation = activationCount

SkipResetIsRejectedWithoutAuthorityMutation ==
    phase = "skip-rejected" =>
        /\ skipResetAttempted
        /\ ~skipResetAccepted
        /\ currentRoot = rejectionRoot
        /\ generation = rejectionGeneration
        /\ activationCount = rejectionActivationCount
        /\ verifiedRoots = rejectionVerifiedRoots
        /\ historyObjects = rejectionHistory
        /\ coldObjects = rejectionCold

ReplayCommitRequiresReactivation ==
    replayCommitted => replayResetObserved /\ ~skipResetAccepted

ReplayPreparationUsesFreshExactBase ==
    replayPrepared =>
        /\ replayResetObserved
        /\ replayBaseRoot = resetRoot
        /\ replayBaseGeneration = resetGeneration

ReplayPreparationLeavesAuthorityUnchanged ==
    phase = "prepared" =>
        /\ currentRoot = prepareRoot
        /\ generation = prepareGeneration
        /\ activationCount = prepareActivationCount
        /\ verifiedRoots = prepareVerifiedRoots
        /\ historyObjects = prepareHistory
        /\ coldObjects = prepareCold

ReplayTargetMatchesPlay ==
    replayCommitted => currentRoot = playTarget /\ replayTarget = playTarget

ReplayReusesAuthenticatedPlayClosure ==
    phase \in {"played", "skip-rejected", "reset", "prepared", "complete"} =>
        /\ historyObjects = playHistory
        /\ coldObjects = playCold

ReplayLifecycleEventuallyCompletes == <>replayCommitted

(***************************************************************************
Negative control: wrongly accept replay directly after B -> T, using the
replayer's stale B@0 base without reactivation or a fresh preparation.
***************************************************************************)
AcceptSkipResetMutant ==
    /\ phase = "played"
    /\ currentRoot = "T"
    /\ skipResetAttempted' = TRUE
    /\ skipResetAccepted' = TRUE
    /\ replayPrepared' = TRUE
    /\ replayBaseRoot' = "B"
    /\ replayBaseGeneration' = 0
    /\ replayTarget' = playTarget
    /\ replayCommitted' = TRUE
    /\ currentRoot' = playTarget
    /\ generation' = generation + 1
    /\ activationCount' = activationCount + 1
    /\ phase' = "complete"
    /\ UNCHANGED <<verifiedRoots, historyObjects, coldObjects, playHistory,
                    playCold, playTarget, rejectionRoot, rejectionGeneration,
                    rejectionActivationCount, rejectionVerifiedRoots,
                    rejectionHistory, rejectionCold, replayResetObserved,
                    resetRoot, resetGeneration, prepareRoot, prepareGeneration,
                    prepareActivationCount, prepareVerifiedRoots,
                    prepareHistory, prepareCold>>

SkipResetAcceptanceMutantNext ==
    PlayCommit \/ AcceptSkipResetMutant \/ ReactivateReplayBase \/
    FreshReplayPrepare \/ ReplayCommit \/ Done
SkipResetAcceptanceMutantSpec ==
    Init /\ [][SkipResetAcceptanceMutantNext]_vars

=============================================================================
