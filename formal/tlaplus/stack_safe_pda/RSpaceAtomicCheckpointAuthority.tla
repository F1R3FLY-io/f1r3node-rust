---------------- MODULE RSpaceAtomicCheckpointAuthority ----------------
EXTENDS FiniteSets, Integers, Naturals, TLC

(***************************************************************************
Finite-state model of the local RSpace checkpoint authority.  A prepared plan
is bound to an exact base (root, generation) and contains immutable history and
cold objects.  A successful activation publishes both object namespaces, root
membership, current root, and generation in one transition.  Invalid or stale
plans fail without changing authoritative state; a no-op changes no bytes and
does not advance generation.
***************************************************************************)

Roots == {"old", "a", "b", "missing"}
Plans == {"noop", "a", "b", "missing", "collision"}
Phases == {"prepared", "committed", "failed"}
HistoryKeys == {"h-old", "h-a", "h-b", "h-missing"}
ColdKeys == {"c-old", "c-a", "c-b", "c-missing"}
Values == {"v-old", "v-a", "v-b", "v-missing", "evil"}

RootHistory(root) == CASE
    root = "old" -> {"h-old"}
    [] root = "a" -> {"h-a"}
    [] root = "b" -> {"h-b"}
    [] root = "missing" -> {"h-missing"}
RootCold(root) == CASE
    root = "old" -> {"c-old"}
    [] root = "a" -> {"c-a"}
    [] root = "b" -> {"c-b"}
    [] root = "missing" -> {"c-missing"}

PlanBaseRoot(plan) == "old"
PlanBaseGeneration(plan) == 0
PlanTarget(plan) == CASE
    plan = "noop" -> "old"
    [] plan = "a" -> "a"
    [] plan = "b" -> "b"
    [] plan = "missing" -> "missing"
    [] plan = "collision" -> "a"
PlanHistory(plan) == CASE
    plan = "noop" -> {}
    [] plan = "a" -> {<<"h-a", "v-a">>}
    [] plan = "b" -> {<<"h-b", "v-b">>}
    [] plan = "missing" -> {}
    [] plan = "collision" -> {<<"h-old", "evil">>, <<"h-a", "v-a">>}
PlanCold(plan) == CASE
    plan = "noop" -> {}
    [] plan = "a" -> {<<"c-a", "v-a">>}
    [] plan = "b" -> {<<"c-b", "v-b">>}
    [] plan = "missing" -> {}
    [] plan = "collision" -> {<<"c-a", "v-a">>}

VARIABLES historyObjects, coldObjects, verifiedRoots, currentRoot, generation,
          phase, acceptedBaseGeneration, activations,
          previousGeneration, lastWasActivation,
          lastFailed, failureHistory, failureCold, failureVerified,
          failureCurrent, failureGeneration,
          lastNoOp, noOpHistory, noOpCold, noOpVerified,
          noOpCurrent, noOpGeneration

Keys(objects) == {pair[1] : pair \in objects}
ContentImmutable(objects) ==
    \A left, right \in objects:
        left[1] = right[1] => left[2] = right[2]
Complete(root, history, cold) ==
    /\ RootHistory(root) \subseteq Keys(history)
    /\ RootCold(root) \subseteq Keys(cold)
ExactBase(plan) ==
    /\ currentRoot = PlanBaseRoot(plan)
    /\ generation = PlanBaseGeneration(plan)
CombinedHistory(plan) == historyObjects \union PlanHistory(plan)
CombinedCold(plan) == coldObjects \union PlanCold(plan)
ValidPreparedPlan(plan) ==
    /\ ContentImmutable(CombinedHistory(plan))
    /\ ContentImmutable(CombinedCold(plan))
    /\ Complete(PlanTarget(plan), CombinedHistory(plan), CombinedCold(plan))

vars == <<historyObjects, coldObjects, verifiedRoots, currentRoot, generation,
          phase, acceptedBaseGeneration, activations,
          previousGeneration, lastWasActivation,
          lastFailed, failureHistory, failureCold, failureVerified,
          failureCurrent, failureGeneration,
          lastNoOp, noOpHistory, noOpCold, noOpVerified,
          noOpCurrent, noOpGeneration>>

Init ==
    /\ historyObjects = {<<"h-old", "v-old">>}
    /\ coldObjects = {<<"c-old", "v-old">>}
    /\ verifiedRoots = {"old"}
    /\ currentRoot = "old"
    /\ generation = 0
    /\ phase = [plan \in Plans |-> "prepared"]
    /\ acceptedBaseGeneration = [plan \in Plans |-> -1]
    /\ activations = 0
    /\ previousGeneration = 0
    /\ lastWasActivation = FALSE
    /\ lastFailed = FALSE
    /\ failureHistory = historyObjects
    /\ failureCold = coldObjects
    /\ failureVerified = verifiedRoots
    /\ failureCurrent = currentRoot
    /\ failureGeneration = generation
    /\ lastNoOp = FALSE
    /\ noOpHistory = historyObjects
    /\ noOpCold = coldObjects
    /\ noOpVerified = verifiedRoots
    /\ noOpCurrent = currentRoot
    /\ noOpGeneration = generation

CommitPrepared(plan) ==
    /\ plan \in {"a", "b"}
    /\ phase[plan] = "prepared"
    /\ ExactBase(plan)
    /\ ValidPreparedPlan(plan)
    /\ historyObjects' = CombinedHistory(plan)
    /\ coldObjects' = CombinedCold(plan)
    /\ verifiedRoots' = verifiedRoots \union {PlanTarget(plan)}
    /\ currentRoot' = PlanTarget(plan)
    /\ generation' = generation + 1
    /\ phase' = [phase EXCEPT ![plan] = "committed"]
    /\ acceptedBaseGeneration' =
         [acceptedBaseGeneration EXCEPT ![plan] = generation]
    /\ activations' = activations + 1
    /\ previousGeneration' = generation
    /\ lastWasActivation' = TRUE
    /\ lastFailed' = FALSE
    /\ lastNoOp' = FALSE
    /\ UNCHANGED <<failureHistory, failureCold, failureVerified,
                    failureCurrent, failureGeneration,
                    noOpHistory, noOpCold, noOpVerified,
                    noOpCurrent, noOpGeneration>>

CommitAnyPrepared == \E plan \in Plans: CommitPrepared(plan)

CommitNoOp ==
    /\ phase["noop"] = "prepared"
    /\ ExactBase("noop")
    /\ phase' = [phase EXCEPT !["noop"] = "committed"]
    /\ acceptedBaseGeneration' =
         [acceptedBaseGeneration EXCEPT !["noop"] = generation]
    /\ previousGeneration' = generation
    /\ lastWasActivation' = FALSE
    /\ lastFailed' = FALSE
    /\ lastNoOp' = TRUE
    /\ noOpHistory' = historyObjects
    /\ noOpCold' = coldObjects
    /\ noOpVerified' = verifiedRoots
    /\ noOpCurrent' = currentRoot
    /\ noOpGeneration' = generation
    /\ UNCHANGED <<historyObjects, coldObjects, verifiedRoots, currentRoot,
                    generation, activations, failureHistory, failureCold,
                    failureVerified, failureCurrent, failureGeneration>>

RejectPlan(plan) ==
    /\ phase[plan] = "prepared"
    /\ plan # "noop"
    /\ (~ExactBase(plan) \/ ~ValidPreparedPlan(plan))
    /\ phase' = [phase EXCEPT ![plan] = "failed"]
    /\ failureHistory' = historyObjects
    /\ failureCold' = coldObjects
    /\ failureVerified' = verifiedRoots
    /\ failureCurrent' = currentRoot
    /\ failureGeneration' = generation
    /\ previousGeneration' = generation
    /\ lastWasActivation' = FALSE
    /\ lastFailed' = TRUE
    /\ lastNoOp' = FALSE
    /\ UNCHANGED <<historyObjects, coldObjects, verifiedRoots, currentRoot,
                    generation, acceptedBaseGeneration, activations,
                    noOpHistory, noOpCold, noOpVerified,
                    noOpCurrent, noOpGeneration>>

RejectAnyPlan == \E plan \in Plans: RejectPlan(plan)

ReactivateOld ==
    /\ currentRoot # "old"
    /\ "old" \in verifiedRoots
    /\ currentRoot' = "old"
    /\ generation' = generation + 1
    /\ activations' = activations + 1
    /\ previousGeneration' = generation
    /\ lastWasActivation' = TRUE
    /\ lastFailed' = FALSE
    /\ lastNoOp' = FALSE
    /\ UNCHANGED <<historyObjects, coldObjects, verifiedRoots, phase,
                    acceptedBaseGeneration, failureHistory, failureCold,
                    failureVerified, failureCurrent, failureGeneration,
                    noOpHistory, noOpCold, noOpVerified,
                    noOpCurrent, noOpGeneration>>

Next == CommitAnyPrepared \/ CommitNoOp \/ RejectAnyPlan \/ ReactivateOld
Spec == Init /\ [][Next]_vars

TypeInvariant ==
    /\ historyObjects \subseteq HistoryKeys \X Values
    /\ coldObjects \subseteq ColdKeys \X Values
    /\ verifiedRoots \subseteq Roots
    /\ currentRoot \in Roots
    /\ generation \in Nat
    /\ phase \in [Plans -> Phases]
    /\ acceptedBaseGeneration \in [Plans -> -1..3]
    /\ activations \in Nat

CurrentRootIsVerified == currentRoot \in verifiedRoots

VerifiedRootsHaveCompleteClosure ==
    \A root \in verifiedRoots: Complete(root, historyObjects, coldObjects)

ContentKeysAreImmutable ==
    /\ ContentImmutable(historyObjects)
    /\ ContentImmutable(coldObjects)

NoUnpublishedCheckpointObjects ==
    /\ Keys(historyObjects) =
         UNION {RootHistory(root) : root \in verifiedRoots}
    /\ Keys(coldObjects) =
         UNION {RootCold(root) : root \in verifiedRoots}

GenerationAdvancesExactlyOnActivation ==
    /\ generation = activations
    /\ IF lastWasActivation
          THEN generation = previousGeneration + 1
          ELSE generation = previousGeneration

CommitUsesExactBase ==
    \A plan \in Plans:
        phase[plan] = "committed" /\ plan # "noop" =>
            acceptedBaseGeneration[plan] = PlanBaseGeneration(plan)

FailedCommitLeavesAuthorityUnchanged ==
    lastFailed =>
        /\ historyObjects = failureHistory
        /\ coldObjects = failureCold
        /\ verifiedRoots = failureVerified
        /\ currentRoot = failureCurrent
        /\ generation = failureGeneration

NoOpLeavesAuthorityUnchanged ==
    lastNoOp =>
        /\ historyObjects = noOpHistory
        /\ coldObjects = noOpCold
        /\ verifiedRoots = noOpVerified
        /\ currentRoot = noOpCurrent
        /\ generation = noOpGeneration

(***************************************************************************
Negative control 1: split object persistence from root publication.  The
intermediate state exposes unreachable local-checkpoint objects.
***************************************************************************)
SplitObjectWriteMutant(plan) ==
    /\ plan \in {"a", "b"}
    /\ phase[plan] = "prepared"
    /\ ExactBase(plan)
    /\ ValidPreparedPlan(plan)
    /\ historyObjects' = CombinedHistory(plan)
    /\ coldObjects' = CombinedCold(plan)
    /\ previousGeneration' = generation
    /\ lastWasActivation' = FALSE
    /\ lastFailed' = FALSE
    /\ lastNoOp' = FALSE
    /\ UNCHANGED <<verifiedRoots, currentRoot, generation, phase,
                    acceptedBaseGeneration, activations,
                    failureHistory, failureCold, failureVerified,
                    failureCurrent, failureGeneration,
                    noOpHistory, noOpCold, noOpVerified,
                    noOpCurrent, noOpGeneration>>

SplitPublicationMutantNext == Next \/
    (\E plan \in Plans: SplitObjectWriteMutant(plan))
SplitPublicationMutantSpec == Init /\ [][SplitPublicationMutantNext]_vars

(***************************************************************************
Negative control 2: compare only the root, not generation.  After A commits
and old is reactivated, B sees the same root at generation 2 and is admitted.
***************************************************************************)
RootOnlyCompareMutant(plan) ==
    /\ plan \in {"a", "b"}
    /\ phase[plan] = "prepared"
    /\ currentRoot = PlanBaseRoot(plan)
    /\ generation # PlanBaseGeneration(plan)
    /\ ValidPreparedPlan(plan)
    /\ historyObjects' = CombinedHistory(plan)
    /\ coldObjects' = CombinedCold(plan)
    /\ verifiedRoots' = verifiedRoots \union {PlanTarget(plan)}
    /\ currentRoot' = PlanTarget(plan)
    /\ generation' = generation + 1
    /\ phase' = [phase EXCEPT ![plan] = "committed"]
    /\ acceptedBaseGeneration' =
         [acceptedBaseGeneration EXCEPT ![plan] = generation]
    /\ activations' = activations + 1
    /\ previousGeneration' = generation
    /\ lastWasActivation' = TRUE
    /\ lastFailed' = FALSE
    /\ lastNoOp' = FALSE
    /\ UNCHANGED <<failureHistory, failureCold, failureVerified,
                    failureCurrent, failureGeneration,
                    noOpHistory, noOpCold, noOpVerified,
                    noOpCurrent, noOpGeneration>>

RootOnlyCompareMutantNext == Next \/
    (\E plan \in Plans: RootOnlyCompareMutant(plan))
RootOnlyCompareMutantSpec == Init /\ [][RootOnlyCompareMutantNext]_vars

(***************************************************************************
Negative control 3: trust a caller's target without requiring its exact root
object or complete cold closure.
***************************************************************************)
TrustCallerTargetMutant ==
    /\ phase["missing"] = "prepared"
    /\ ExactBase("missing")
    /\ verifiedRoots' = verifiedRoots \union {"missing"}
    /\ currentRoot' = "missing"
    /\ generation' = generation + 1
    /\ phase' = [phase EXCEPT !["missing"] = "committed"]
    /\ acceptedBaseGeneration' =
         [acceptedBaseGeneration EXCEPT !["missing"] = generation]
    /\ activations' = activations + 1
    /\ previousGeneration' = generation
    /\ lastWasActivation' = TRUE
    /\ lastFailed' = FALSE
    /\ lastNoOp' = FALSE
    /\ UNCHANGED <<historyObjects, coldObjects, failureHistory, failureCold,
                    failureVerified, failureCurrent, failureGeneration,
                    noOpHistory, noOpCold, noOpVerified,
                    noOpCurrent, noOpGeneration>>

TrustCallerTargetMutantNext == Next \/ TrustCallerTargetMutant
TrustCallerTargetMutantSpec == Init /\ [][TrustCallerTargetMutantNext]_vars

=============================================================================
