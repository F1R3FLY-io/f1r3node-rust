-------------- MODULE RSpaceRecordReplayAuthorityLifecycle --------------
EXTENDS Integers, Naturals, TLC

(***************************************************************************
Lifecycle refinement for recording and replaying the same RSpace transition
through one shared atomic checkpoint authority.

The recorder starts at B and publishes T.  A foreign commit may then publish
F once: before the replayer resets, while it is reset, or while it is
prepared.  The replayer resets its own view to the recorded base B, which
changes no authority variable, prepares B -> T from that base, and publishes
T, the successor of its recorded base, at the current generation plus one,
whichever of T and F is current.  Reset does not reactivate B, and the
replay commit does not require B to be current.

The replayer's reset and preparation write nothing to the shared authority,
but a foreign commit may move the authority while the replayer is reset or
prepared.  ReplayResetLeavesAuthorityUnchanged and
ReplayPreparationLeavesAuthorityUnchanged are therefore action properties of
the replayer's own reset and preparation steps, not comparisons with a
snapshot held for a whole phase.  A step is the replayer's reset (or
preparation) when it sets the replayer's own flag, whichever action takes it.

SkipResetMutantSpec is the first negative control: the replayer skips the
reset and prepares from the current root instead of its recorded base, and
must violate ReplayPreparationUsesRecordedBase.  ResetMovesAuthorityMutantSpec
is the second: the replayer's reset moves the current root back to B, and
must violate the action property ReplayResetLeavesAuthorityUnchanged.  The
three Never... statements are reachability witnesses, each of which must be
violated.
***************************************************************************)

Roots == {"B", "T", "F"}
Phases == {"initial", "played", "reset", "prepared", "complete"}
HistoryKeys == {"h-B", "h-T", "h-F"}
ColdKeys == {"c-B", "c-T", "c-F"}
Values == {"v-B", "v-T", "v-F"}

RootHistory(root) == CASE
    root = "B" -> {"h-B"}
    [] root = "T" -> {"h-T"}
    [] root = "F" -> {"h-F"}
RootCold(root) == CASE
    root = "B" -> {"c-B"}
    [] root = "T" -> {"c-T"}
    [] root = "F" -> {"c-F"}
Keys(objects) == {pair[1] : pair \in objects}
ContentImmutable(objects) ==
    \A left, right \in objects:
        left[1] = right[1] => left[2] = right[2]
Complete(root, history, cold) ==
    /\ RootHistory(root) \subseteq Keys(history)
    /\ RootCold(root) \subseteq Keys(cold)

VARIABLES phase, currentRoot, generation, activationCount,
          verifiedRoots, historyObjects, coldObjects,
          recordedBase, playHistory, playCold, playTarget, foreignPublished,
          replayResetObserved, replayViewRoot, resetRoot,
          replayPrepared, replayBaseRoot, replayTarget, prepareRoot,
          replayCommitted, commitRoot, commitGeneration

vars == <<phase, currentRoot, generation, activationCount,
          verifiedRoots, historyObjects, coldObjects,
          recordedBase, playHistory, playCold, playTarget, foreignPublished,
          replayResetObserved, replayViewRoot, resetRoot,
          replayPrepared, replayBaseRoot, replayTarget, prepareRoot,
          replayCommitted, commitRoot, commitGeneration>>

AuthorityVars == <<currentRoot, generation, activationCount, verifiedRoots,
                   historyObjects, coldObjects>>

ForeignHistory == IF foreignPublished THEN {<<"h-F", "v-F">>} ELSE {}
ForeignCold == IF foreignPublished THEN {<<"c-F", "v-F">>} ELSE {}

Init ==
    /\ phase = "initial"
    /\ currentRoot = "B"
    /\ generation = 0
    /\ activationCount = 0
    /\ verifiedRoots = {"B"}
    /\ historyObjects = {<<"h-B", "v-B">>}
    /\ coldObjects = {<<"c-B", "v-B">>}
    /\ recordedBase = currentRoot
    /\ playHistory = historyObjects
    /\ playCold = coldObjects
    /\ playTarget = "T"
    /\ foreignPublished = FALSE
    /\ replayResetObserved = FALSE
    /\ replayViewRoot = currentRoot
    /\ resetRoot = currentRoot
    /\ replayPrepared = FALSE
    /\ replayBaseRoot = currentRoot
    /\ replayTarget = "T"
    /\ prepareRoot = currentRoot
    /\ replayCommitted = FALSE
    /\ commitRoot = currentRoot
    /\ commitGeneration = generation

PlayCommit ==
    /\ phase = "initial"
    /\ recordedBase \in verifiedRoots
    /\ historyObjects' = historyObjects \union {<<"h-T", "v-T">>}
    /\ coldObjects' = coldObjects \union {<<"c-T", "v-T">>}
    /\ verifiedRoots' = verifiedRoots \union {"T"}
    /\ currentRoot' = "T"
    /\ generation' = generation + 1
    /\ activationCount' = activationCount + 1
    /\ playHistory' = historyObjects'
    /\ playCold' = coldObjects'
    /\ phase' = "played"
    /\ UNCHANGED <<recordedBase, playTarget, foreignPublished,
                    replayResetObserved, replayViewRoot, resetRoot,
                    replayPrepared, replayBaseRoot, replayTarget, prepareRoot,
                    replayCommitted, commitRoot, commitGeneration>>

ForeignCommit ==
    /\ phase \in {"played", "reset", "prepared"}
    /\ ~foreignPublished
    /\ historyObjects' = historyObjects \union {<<"h-F", "v-F">>}
    /\ coldObjects' = coldObjects \union {<<"c-F", "v-F">>}
    /\ verifiedRoots' = verifiedRoots \union {"F"}
    /\ currentRoot' = "F"
    /\ generation' = generation + 1
    /\ activationCount' = activationCount + 1
    /\ foreignPublished' = TRUE
    /\ UNCHANGED <<phase, recordedBase, playHistory, playCold, playTarget,
                    replayResetObserved, replayViewRoot, resetRoot,
                    replayPrepared, replayBaseRoot, replayTarget, prepareRoot,
                    replayCommitted, commitRoot, commitGeneration>>

ReplayReset ==
    /\ phase = "played"
    /\ recordedBase \in verifiedRoots
    /\ replayResetObserved' = TRUE
    /\ replayViewRoot' = recordedBase
    /\ resetRoot' = currentRoot
    /\ phase' = "reset"
    /\ UNCHANGED <<currentRoot, generation, activationCount, verifiedRoots,
                    historyObjects, coldObjects, recordedBase, playHistory,
                    playCold, playTarget, foreignPublished, replayPrepared,
                    replayBaseRoot, replayTarget, prepareRoot,
                    replayCommitted, commitRoot, commitGeneration>>

ReplayPrepare ==
    /\ phase = "reset"
    /\ replayPrepared' = TRUE
    /\ replayBaseRoot' = replayViewRoot
    /\ replayTarget' = playTarget
    /\ prepareRoot' = currentRoot
    /\ phase' = "prepared"
    /\ UNCHANGED <<currentRoot, generation, activationCount, verifiedRoots,
                    historyObjects, coldObjects, recordedBase, playHistory,
                    playCold, playTarget, foreignPublished,
                    replayResetObserved, replayViewRoot, resetRoot,
                    replayCommitted, commitRoot, commitGeneration>>

ReplayCommit ==
    /\ phase = "prepared"
    /\ replayPrepared
    /\ replayBaseRoot \in verifiedRoots
    /\ Complete(replayTarget, historyObjects, coldObjects)
    /\ verifiedRoots' = verifiedRoots \union {replayTarget}
    /\ currentRoot' = replayTarget
    /\ generation' = generation + 1
    /\ activationCount' = activationCount + 1
    /\ commitRoot' = currentRoot
    /\ commitGeneration' = generation
    /\ replayCommitted' = TRUE
    /\ phase' = "complete"
    /\ UNCHANGED <<historyObjects, coldObjects, recordedBase, playHistory,
                    playCold, playTarget, foreignPublished,
                    replayResetObserved, replayViewRoot, resetRoot,
                    replayPrepared, replayBaseRoot, replayTarget, prepareRoot>>

Done == phase = "complete" /\ UNCHANGED vars

Next == PlayCommit \/ ForeignCommit \/ ReplayReset \/ ReplayPrepare \/
        ReplayCommit \/ Done
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeInvariant ==
    /\ phase \in Phases
    /\ currentRoot \in Roots
    /\ generation \in Nat
    /\ activationCount \in Nat
    /\ verifiedRoots \subseteq Roots
    /\ historyObjects \subseteq HistoryKeys \X Values
    /\ coldObjects \subseteq ColdKeys \X Values
    /\ recordedBase \in Roots
    /\ foreignPublished \in BOOLEAN
    /\ replayViewRoot \in Roots
    /\ replayBaseRoot \in Roots
    /\ resetRoot \in Roots
    /\ prepareRoot \in Roots
    /\ commitRoot \in Roots
    /\ commitGeneration \in Nat

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

ReplayResetStep == ~replayResetObserved /\ replayResetObserved'

ReplayPreparationStep == ~replayPrepared /\ replayPrepared'

ReplayResetLeavesAuthorityUnchanged ==
    [][ReplayResetStep => UNCHANGED AuthorityVars]_vars

ReplayCommitPublishesSuccessorOfRecordedBase ==
    replayCommitted =>
        /\ replayResetObserved
        /\ replayBaseRoot = recordedBase
        /\ replayTarget = playTarget
        /\ currentRoot = replayTarget
        /\ generation = commitGeneration + 1

ReplayPreparationUsesRecordedBase ==
    replayPrepared =>
        /\ replayResetObserved
        /\ replayBaseRoot = recordedBase

ReplayPreparationLeavesAuthorityUnchanged ==
    [][ReplayPreparationStep => UNCHANGED AuthorityVars]_vars

ReplayTargetMatchesPlay ==
    replayCommitted => currentRoot = playTarget /\ replayTarget = playTarget

ReplayReusesAuthenticatedPlayClosure ==
    phase \in {"played", "reset", "prepared", "complete"} =>
        /\ historyObjects = playHistory \union ForeignHistory
        /\ coldObjects = playCold \union ForeignCold

ReplayLifecycleEventuallyCompletes == <>replayCommitted

(***************************************************************************
Reachability witnesses.  Only ForeignCommit publishes F, so the first two
are a foreign commit while the replayer is reset and while it is prepared,
and the third is a replay commit made with F current after a reset made
with T current.  Each must be violated.
***************************************************************************)
NeverForeignCommitWhileReplayReset ==
    ~(phase = "reset" /\ resetRoot = "T" /\ currentRoot = "F")

NeverForeignCommitWhileReplayPrepared ==
    ~(phase = "prepared" /\ prepareRoot = "T" /\ currentRoot = "F")

NeverReplayCommitAcrossForeignCommit ==
    ~(replayCommitted /\ resetRoot = "T" /\ commitRoot = "F")

(***************************************************************************
Negative control 1: skip the reset and prepare from the current root (T, or
F after the foreign commit) instead of the recorded base B.
***************************************************************************)
SkipResetPrepareFromCurrentMutant ==
    /\ phase = "played"
    /\ replayPrepared' = TRUE
    /\ replayBaseRoot' = currentRoot
    /\ replayTarget' = playTarget
    /\ prepareRoot' = currentRoot
    /\ phase' = "prepared"
    /\ UNCHANGED <<currentRoot, generation, activationCount, verifiedRoots,
                    historyObjects, coldObjects, recordedBase, playHistory,
                    playCold, playTarget, foreignPublished,
                    replayResetObserved, replayViewRoot, resetRoot,
                    replayCommitted, commitRoot, commitGeneration>>

SkipResetMutantNext == Next \/ SkipResetPrepareFromCurrentMutant
SkipResetMutantSpec == Init /\ [][SkipResetMutantNext]_vars

(***************************************************************************
Negative control 2: the replayer's reset activates the recorded base B on the
shared authority, moving the current root and advancing the generation,
instead of only attaching the replayer's own view to B.
***************************************************************************)
ResetMovesAuthorityMutant ==
    /\ phase = "played"
    /\ recordedBase \in verifiedRoots
    /\ currentRoot' = recordedBase
    /\ generation' = generation + 1
    /\ activationCount' = activationCount + 1
    /\ replayResetObserved' = TRUE
    /\ replayViewRoot' = recordedBase
    /\ resetRoot' = currentRoot
    /\ phase' = "reset"
    /\ UNCHANGED <<verifiedRoots, historyObjects, coldObjects, recordedBase,
                    playHistory, playCold, playTarget, foreignPublished,
                    replayPrepared, replayBaseRoot, replayTarget, prepareRoot,
                    replayCommitted, commitRoot, commitGeneration>>

ResetMovesAuthorityMutantNext == Next \/ ResetMovesAuthorityMutant
ResetMovesAuthorityMutantSpec == Init /\ [][ResetMovesAuthorityMutantNext]_vars

=============================================================================
