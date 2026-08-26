-------------------- MODULE RSpaceAffineImportLifecycle --------------------
EXTENDS FiniteSets, Naturals, TLC

(***************************************************************************
Finite-state model of the affine RSpace import capability and verified-root
catalog. Two activation sessions race from the same observed generation while
one horizon session preserves the current root. Page staging is private;
sealing consumes staging authority; publication is one-shot; and generation
CAS admits at most one of the racing activators.
***************************************************************************)

Sessions == {"activate-a", "activate-b", "preserve"}
Roots == {"old", "new"}
Phases == {"open", "sealed", "published", "rejected"}

Mode(session) == IF session = "preserve" THEN "preserve" ELSE "activate"
ExpectedGeneration(session) == 0

VARIABLES phase, pendingPages, closureVerified, sealObserved,
          catalog, currentRoot, generation, preserveMoves

vars == <<phase, pendingPages, closureVerified, sealObserved,
          catalog, currentRoot, generation, preserveMoves>>

Init ==
    /\ phase = [session \in Sessions |-> "open"]
    /\ pendingPages = [session \in Sessions |-> 1]
    /\ closureVerified = [session \in Sessions |-> FALSE]
    /\ sealObserved = [session \in Sessions |-> FALSE]
    /\ catalog = {"old"}
    /\ currentRoot = "old"
    /\ generation = 0
    /\ preserveMoves = 0

StageTerminalPage(session) ==
    /\ phase[session] = "open"
    /\ pendingPages[session] = 1
    /\ pendingPages' = [pendingPages EXCEPT ![session] = 0]
    /\ closureVerified' = [closureVerified EXCEPT ![session] = TRUE]
    /\ UNCHANGED <<phase, sealObserved, catalog, currentRoot, generation,
                    preserveMoves>>

StageAnyTerminalPage ==
    \E session \in Sessions: StageTerminalPage(session)

SealComplete(session) ==
    /\ phase[session] = "open"
    /\ pendingPages[session] = 0
    /\ closureVerified[session]
    /\ phase' = [phase EXCEPT ![session] = "sealed"]
    /\ sealObserved' = [sealObserved EXCEPT ![session] = TRUE]
    /\ UNCHANGED <<pendingPages, closureVerified, catalog, currentRoot,
                    generation, preserveMoves>>

SealAnyComplete == \E session \in Sessions: SealComplete(session)

PublishPreservingCurrent(session) ==
    /\ phase[session] = "sealed"
    /\ Mode(session) = "preserve"
    /\ phase' = [phase EXCEPT ![session] = "published"]
    /\ catalog' = catalog \union {"new"}
    /\ UNCHANGED <<pendingPages, closureVerified, sealObserved, currentRoot,
                    generation, preserveMoves>>

PublishAnyPreservingCurrent ==
    \E session \in Sessions: PublishPreservingCurrent(session)

PublishActivation(session) ==
    /\ phase[session] = "sealed"
    /\ Mode(session) = "activate"
    /\ generation = ExpectedGeneration(session)
    /\ phase' = [phase EXCEPT ![session] = "published"]
    /\ catalog' = catalog \union {"new"}
    /\ currentRoot' = "new"
    /\ generation' = generation + 1
    /\ UNCHANGED <<pendingPages, closureVerified, sealObserved, preserveMoves>>

PublishAnyActivation == \E session \in Sessions: PublishActivation(session)

RejectStaleActivation(session) ==
    /\ phase[session] = "sealed"
    /\ Mode(session) = "activate"
    /\ generation # ExpectedGeneration(session)
    /\ phase' = [phase EXCEPT ![session] = "rejected"]
    /\ UNCHANGED <<pendingPages, closureVerified, sealObserved, catalog,
                    currentRoot, generation, preserveMoves>>

RejectAnyStaleActivation ==
    \E session \in Sessions: RejectStaleActivation(session)

Next == StageAnyTerminalPage \/ SealAnyComplete
        \/ PublishAnyPreservingCurrent \/ PublishAnyActivation
        \/ RejectAnyStaleActivation

Spec == Init /\ [][Next]_vars

TypeInvariant ==
    /\ phase \in [Sessions -> Phases]
    /\ pendingPages \in [Sessions -> 0..1]
    /\ closureVerified \in [Sessions -> BOOLEAN]
    /\ sealObserved \in [Sessions -> BOOLEAN]
    /\ catalog \subseteq Roots
    /\ currentRoot \in Roots
    /\ generation \in 0..1
    /\ preserveMoves \in Nat

SealRequiresCompleteClosure ==
    \A session \in Sessions:
        phase[session] \in {"sealed", "published", "rejected"} =>
            /\ pendingPages[session] = 0
            /\ closureVerified[session]
            /\ sealObserved[session]

PublicationRequiresConsumedSeal ==
    \A session \in Sessions:
        phase[session] = "published" =>
            /\ sealObserved[session]
            /\ closureVerified[session]
            /\ pendingPages[session] = 0

CurrentRootIsVerified == currentRoot \in catalog

AtMostOneRacingActivationPublishes ==
    Cardinality({session \in Sessions:
        /\ Mode(session) = "activate"
        /\ phase[session] = "published"}) <= 1

ActivationGenerationIsCASProtected ==
    generation = Cardinality({session \in Sessions:
        /\ Mode(session) = "activate"
        /\ phase[session] = "published"})

PreserveCurrentNeverMovesAuthority == preserveMoves = 0

(***************************************************************************
Negative control: publish directly from an incomplete open session.
***************************************************************************)
PublishIncompleteMutant(session) ==
    /\ phase[session] = "open"
    /\ pendingPages[session] = 1
    /\ ~closureVerified[session]
    /\ phase' = [phase EXCEPT ![session] = "published"]
    /\ catalog' = catalog \union {"new"}
    /\ UNCHANGED <<pendingPages, closureVerified, sealObserved, currentRoot,
                    generation, preserveMoves>>

IncompleteMutantNext == Next \/
    (\E session \in Sessions: PublishIncompleteMutant(session))
IncompleteMutantSpec == Init /\ [][IncompleteMutantNext]_vars

(***************************************************************************
Negative control: ignore generation mismatch and publish a stale activator.
***************************************************************************)
PublishStaleActivationMutant(session) ==
    /\ phase[session] = "sealed"
    /\ Mode(session) = "activate"
    /\ generation # ExpectedGeneration(session)
    /\ phase' = [phase EXCEPT ![session] = "published"]
    /\ catalog' = catalog \union {"new"}
    /\ currentRoot' = "new"
    /\ generation' = generation + 1
    /\ UNCHANGED <<pendingPages, closureVerified, sealObserved, preserveMoves>>

StaleActivationMutantNext == Next \/
    (\E session \in Sessions: PublishStaleActivationMutant(session))
StaleActivationMutantSpec == Init /\ [][StaleActivationMutantNext]_vars

(***************************************************************************
Negative control: preserving a verified horizon root also moves current-root
authority and its generation.
***************************************************************************)
MoveCurrentOnPreserveMutant(session) ==
    /\ phase[session] = "sealed"
    /\ Mode(session) = "preserve"
    /\ phase' = [phase EXCEPT ![session] = "published"]
    /\ catalog' = catalog \union {"new"}
    /\ currentRoot' = "new"
    /\ generation' = generation + 1
    /\ preserveMoves' = preserveMoves + 1
    /\ UNCHANGED <<pendingPages, closureVerified, sealObserved>>

PreserveMutationMutantNext == Next \/
    (\E session \in Sessions: MoveCurrentOnPreserveMutant(session))
PreserveMutationMutantSpec == Init /\ [][PreserveMutationMutantNext]_vars

=============================================================================
