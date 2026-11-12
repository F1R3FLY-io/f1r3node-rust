---------------- MODULE RSpaceAtomicCheckpointAuthority ----------------
EXTENDS FiniteSets, Integers, Naturals, TLC

(***************************************************************************
Finite-state model of the local RSpace checkpoint authority.  A prepared plan
names the root its view was prepared from (its base) and contains immutable
history and cold objects.  A valid plan whose base is a verified root publishes
its target whatever root is current, at the current generation plus one; both
object namespaces, root membership, current root, and generation change in one
transition.  Invalid plans and plans whose base is not verified fail without
changing authoritative state; a no-op changes no bytes and does not advance
generation.  The plan unverified is prepared from the root unknown, which is
never verified, so its refusal is reachable.
***************************************************************************)

Roots == {"old", "a", "b", "missing", "unknown"}
Plans == {"noop", "a", "b", "missing", "collision", "unverified"}
Phases == {"prepared", "committed", "failed"}
HistoryKeys == {"h-old", "h-a", "h-b", "h-missing", "h-unknown"}
ColdKeys == {"c-old", "c-a", "c-b", "c-missing", "c-unknown"}
Values == {"v-old", "v-a", "v-b", "v-missing", "evil"}

RootHistory(root) == CASE
    root = "old" -> {"h-old"}
    [] root = "a" -> {"h-a"}
    [] root = "b" -> {"h-b"}
    [] root = "missing" -> {"h-missing"}
    [] root = "unknown" -> {"h-unknown"}
RootCold(root) == CASE
    root = "old" -> {"c-old"}
    [] root = "a" -> {"c-a"}
    [] root = "b" -> {"c-b"}
    [] root = "missing" -> {"c-missing"}
    [] root = "unknown" -> {"c-unknown"}

PlanBaseRoot(plan) == IF plan = "unverified" THEN "unknown" ELSE "old"
PlanTarget(plan) == CASE
    plan = "noop" -> "old"
    [] plan = "a" -> "a"
    [] plan = "b" -> "b"
    [] plan = "missing" -> "missing"
    [] plan = "collision" -> "a"
    [] plan = "unverified" -> "a"
PlanHistory(plan) == CASE
    plan = "noop" -> {}
    [] plan = "a" -> {<<"h-a", "v-a">>}
    [] plan = "b" -> {<<"h-b", "v-b">>}
    [] plan = "missing" -> {}
    [] plan = "collision" -> {<<"h-old", "evil">>, <<"h-a", "v-a">>}
    [] plan = "unverified" -> {<<"h-a", "v-a">>}
PlanCold(plan) == CASE
    plan = "noop" -> {}
    [] plan = "a" -> {<<"c-a", "v-a">>}
    [] plan = "b" -> {<<"c-b", "v-b">>}
    [] plan = "missing" -> {}
    [] plan = "collision" -> {<<"c-a", "v-a">>}
    [] plan = "unverified" -> {<<"c-a", "v-a">>}

VARIABLES historyObjects, coldObjects, verifiedRoots, currentRoot, generation,
          phase, activations,
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
VerifiedBase(plan) == PlanBaseRoot(plan) \in verifiedRoots
CombinedHistory(plan) == historyObjects \union PlanHistory(plan)
CombinedCold(plan) == coldObjects \union PlanCold(plan)
ValidPreparedPlan(plan) ==
    /\ ContentImmutable(CombinedHistory(plan))
    /\ ContentImmutable(CombinedCold(plan))
    /\ Complete(PlanTarget(plan), CombinedHistory(plan), CombinedCold(plan))

vars == <<historyObjects, coldObjects, verifiedRoots, currentRoot, generation,
          phase, activations,
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
    /\ plan \in {"a", "b", "unverified"}
    /\ phase[plan] = "prepared"
    /\ VerifiedBase(plan)
    /\ ValidPreparedPlan(plan)
    /\ historyObjects' = CombinedHistory(plan)
    /\ coldObjects' = CombinedCold(plan)
    /\ verifiedRoots' = verifiedRoots \union {PlanTarget(plan)}
    /\ currentRoot' = PlanTarget(plan)
    /\ generation' = generation + 1
    /\ phase' = [phase EXCEPT ![plan] = "committed"]
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
    /\ VerifiedBase("noop")
    /\ phase' = [phase EXCEPT !["noop"] = "committed"]
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
    /\ (~VerifiedBase(plan) \/ ~ValidPreparedPlan(plan))
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
                    generation, activations,
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
                    failureHistory, failureCold,
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

VerifiedBasePlanIsNeverRefused ==
    \A plan \in Plans:
        phase[plan] = "failed" =>
            ~ValidPreparedPlan(plan) \/ ~VerifiedBase(plan)

UnverifiedBasePlanIsNeverPublished ==
    \A plan \in Plans:
        phase[plan] = "committed" => VerifiedBase(plan)

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
Reachability witness: the plan prepared from the unverified root unknown is
refused.  RSpaceAtomicCheckpointAuthorityUnverifiedBaseRefusalWitness.cfg
checks this statement and is expected to be violated.
***************************************************************************)
NeverUnverifiedBaseRefusal == phase["unverified"] # "failed"

(***************************************************************************
Negative control 1: split object persistence from root publication.  The
intermediate state exposes unreachable local-checkpoint objects.
***************************************************************************)
SplitObjectWriteMutant(plan) ==
    /\ plan \in {"a", "b"}
    /\ phase[plan] = "prepared"
    /\ VerifiedBase(plan)
    /\ ValidPreparedPlan(plan)
    /\ historyObjects' = CombinedHistory(plan)
    /\ coldObjects' = CombinedCold(plan)
    /\ previousGeneration' = generation
    /\ lastWasActivation' = FALSE
    /\ lastFailed' = FALSE
    /\ lastNoOp' = FALSE
    /\ UNCHANGED <<verifiedRoots, currentRoot, generation, phase,
                    activations,
                    failureHistory, failureCold, failureVerified,
                    failureCurrent, failureGeneration,
                    noOpHistory, noOpCold, noOpVerified,
                    noOpCurrent, noOpGeneration>>

SplitPublicationMutantNext == Next \/
    (\E plan \in Plans: SplitObjectWriteMutant(plan))
SplitPublicationMutantSpec == Init /\ [][SplitPublicationMutantNext]_vars

(***************************************************************************
Negative control 2: treat the current root as the base every view shares and
refuse a valid plan whose verified base is no longer current.  After A
commits, B's base old is still verified but not current, and B is refused.
***************************************************************************)
SharedCurrentBaseMutant(plan) ==
    /\ plan \in {"a", "b"}
    /\ phase[plan] = "prepared"
    /\ VerifiedBase(plan)
    /\ ValidPreparedPlan(plan)
    /\ currentRoot # PlanBaseRoot(plan)
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
                    generation, activations,
                    noOpHistory, noOpCold, noOpVerified,
                    noOpCurrent, noOpGeneration>>

SharedCurrentBaseMutantNext == Next \/
    (\E plan \in Plans: SharedCurrentBaseMutant(plan))
SharedCurrentBaseMutantSpec == Init /\ [][SharedCurrentBaseMutantNext]_vars

(***************************************************************************
Negative control 3: trust a caller's target without requiring its exact root
object or complete cold closure.
***************************************************************************)
TrustCallerTargetMutant ==
    /\ phase["missing"] = "prepared"
    /\ VerifiedBase("missing")
    /\ verifiedRoots' = verifiedRoots \union {"missing"}
    /\ currentRoot' = "missing"
    /\ generation' = generation + 1
    /\ phase' = [phase EXCEPT !["missing"] = "committed"]
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

(***************************************************************************
Negative control 4: drop the verified-base guard of CommitPrepared.  The plan
prepared from the unverified root unknown then publishes its target.
***************************************************************************)
UnverifiedBaseCommitMutant(plan) ==
    /\ plan \in {"a", "b", "unverified"}
    /\ phase[plan] = "prepared"
    /\ ValidPreparedPlan(plan)
    /\ historyObjects' = CombinedHistory(plan)
    /\ coldObjects' = CombinedCold(plan)
    /\ verifiedRoots' = verifiedRoots \union {PlanTarget(plan)}
    /\ currentRoot' = PlanTarget(plan)
    /\ generation' = generation + 1
    /\ phase' = [phase EXCEPT ![plan] = "committed"]
    /\ activations' = activations + 1
    /\ previousGeneration' = generation
    /\ lastWasActivation' = TRUE
    /\ lastFailed' = FALSE
    /\ lastNoOp' = FALSE
    /\ UNCHANGED <<failureHistory, failureCold, failureVerified,
                    failureCurrent, failureGeneration,
                    noOpHistory, noOpCold, noOpVerified,
                    noOpCurrent, noOpGeneration>>

UnverifiedBaseCommitMutantNext == Next \/
    (\E plan \in Plans: UnverifiedBaseCommitMutant(plan))
UnverifiedBaseCommitMutantSpec == Init /\ [][UnverifiedBaseCommitMutantNext]_vars

=============================================================================
